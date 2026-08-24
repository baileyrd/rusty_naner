//! Portable MSVC compiler/linker + Windows SDK, installed without the
//! standard `vs_buildtools.exe` bootstrapper -- which needs admin no matter
//! what `--installPath` is given, since it registers itself with the VS
//! Installer service and writes MSI-based state machine-wide regardless of
//! where the toolset itself lands.
//!
//! Instead this fetches the individual payloads that bootstrapper itself
//! would fetch -- VC++ Tools as plain-zip VSIX, Windows SDK components as
//! MSI + external CAB -- and extracts them directly. Same no-admin
//! technique as `mmozeiko/portable-msvc` and
//! `Data-Oriented-House/PortableBuildTools`.
//!
//! Every URL/hash below is pinned from the live VS 17.14 (August 2026)
//! channel manifest (`aka.ms/vs/17/release/channel` -> the
//! `Microsoft.VisualStudio.Manifests.VisualStudio` channel item's
//! `VisualStudio.vsman`, ~18 MB of package metadata). The VC++ Tools
//! packages carry their payload URL/SHA-256 directly. The Windows SDK
//! (`Win11SDK_10.0.26100`) does not: its own top-level payload list is 229
//! anonymous content-hashed `.cab` files with no package names attached --
//! those names exist only in a Burn manifest embedded inside
//! `winsdksetup.exe` itself (extract the exe as a cabinet; its unnamed
//! first member is `BurnManifest.xml`, whose `<MsiPackage Id="package_...">`
//! elements map a name to `<PayloadRef>`s that resolve, via the sibling
//! `<Payload Id="..." FilePath="Installers\\<hash>.cab">` elements, to the
//! exact hashed files the channel manifest publishes).
//!
//! Refreshing these pins means repeating that walk by hand -- there is no
//! `golang-api`/`nodejs-api`-style "latest" endpoint for either half, so
//! this vendor is not wired into the generic resolver/`refresh-pins`
//! machinery. It is dispatched by vendor key in
//! `installer::UnifiedVendorInstaller::install_vendor_inner`, the
//! same "one documented exception" shape `archives::run_exe_installer`'s
//! `RUSTUP_HOME`/`CARGO_HOME` special case already established for a vendor
//! whose install shape the generic single-artifact pipeline cannot express.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::checksum::{self, ChecksumInfo};
use crate::http::Http;
use crate::logger;

/// Append `PROPERTY=value` to `msiexec`'s command line, quoting only the
/// *value* half.
///
/// `Command::arg` always quotes (or doesn't) the *entire* token it's given;
/// it has no way to quote just part of one. That is fine for every other
/// Windows program, which expects `CommandLineToArgvW`-style whole-token
/// quoting -- but msiexec's own parser rejects a whole-token-quoted
/// `"PROPERTY=value"` outright with ERROR_INVALID_COMMAND_LINE (1639) and
/// only accepts `PROPERTY="value"`. `raw_arg` bypasses `Command::arg`'s
/// quoting so the value can be quoted correctly by hand; a trailing
/// backslash is doubled first since it would otherwise escape the closing
/// quote instead of terminating it (`KITSROOT`'s value always ends in one).
#[cfg(windows)]
fn push_msi_property(command: &mut Command, property: &str, value: &str) {
    use std::os::windows::process::CommandExt;
    let escaped = if let Some(stripped) = value.strip_suffix('\\') {
        format!("{stripped}\\\\")
    } else {
        value.to_string()
    };
    command.raw_arg(format!("{property}=\"{escaped}\""));
}

#[cfg(not(windows))]
fn push_msi_property(command: &mut Command, property: &str, value: &str) {
    command.arg(format!("{property}={value}"));
}

/// One downloadable file: a VC++ Tools VSIX (plain zip) or a Windows SDK
/// MSI/external-CAB payload. `sha256` is the digest Microsoft's own
/// manifest publishes for this exact file.
struct Payload {
    file_name: &'static str,
    url: &'static str,
    sha256: &'static str,
}

/// A Windows SDK component: one MSI plus the external `.cab` files its
/// Media table references. `msiexec /a` resolves those relative to the
/// MSI's own directory, so they must be downloaded flat alongside it (see
/// `fetch_msi_component`).
struct MsiComponent {
    label: &'static str,
    msi: Payload,
    cabs: &'static [Payload],
}

/// The MSVC toolset version every VC++ Tools VSIX below extracts under
/// (`Contents\VC\Tools\MSVC\<this>\...`) -- confirmed by listing each
/// VSIX directly; it does not match any single package's own `version`
/// field (those run 14.44.35220-14.44.35228, all sharing this folder).
pub(crate) const MSVC_TOOLSET_VERSION: &str = "14.44.35207";

/// The Windows SDK version every SDK MSI below installs under
/// (`Windows Kits\10\{Include,Lib,bin}\<this>\...`) -- the SDK's
/// long-standing `<major>.<minor>.<build>.0` folder convention, independent
/// of the fuller `10.1.26100.7705` shown in Programs & Features.
pub(crate) const SDK_VERSION: &str = "10.0.26100.0";

/// VC++ Tools: plain-zip VSIX packages. Each extracts a
/// `Contents\VC\Tools\MSVC\<MSVC_TOOLSET_VERSION>\` subtree that every
/// package here shares -- union them onto one target.
const VC_PACKAGES: &[Payload] = &[
    // compiler + linker: cl.exe, link.exe, lib.exe, nmake.exe, ...
    Payload {
        file_name: "Microsoft.VC.14.44.17.14.Tools.HostX64.TargetX64.base.vsix",
        url: "https://download.visualstudio.microsoft.com/download/pr/bbc72d8e-2acd-4229-8f6a-85e23c5e3456/ee0baaa3a112d255f19f6c27dcc0ff6e496949eb9f1f37be0ac908c562a7076c/Microsoft.VC.14.44.17.14.Tools.HostX64.TargetX64.base.vsix",
        sha256: "ee0baaa3a112d255f19f6c27dcc0ff6e496949eb9f1f37be0ac908c562a7076c",
    },
    // en-US localized resource DLLs cl.exe/link.exe need to start at all
    Payload {
        file_name: "Microsoft.VC.14.44.17.14.Tools.HostX64.TargetX64.Res.base.enu.vsix",
        url: "https://download.visualstudio.microsoft.com/download/pr/bbc72d8e-2acd-4229-8f6a-85e23c5e3456/6e31f47833bfa585f56d55716a1ef081f1434f93ad77160eab49c6e193765832/Microsoft.VC.14.44.17.14.Tools.HostX64.TargetX64.Res.base.enu.vsix",
        sha256: "6e31f47833bfa585f56d55716a1ef081f1434f93ad77160eab49c6e193765832",
    },
    // CRT headers
    Payload {
        file_name: "Microsoft.VC.14.44.17.14.CRT.Headers.base.vsix",
        url: "https://download.visualstudio.microsoft.com/download/pr/c610cd8c-801b-44b8-a80a-82cc382aeb43/852382a9aa73502b7849c1bcadfb603ba7175c4e8b60e6aba03c7de711d4ece5/Microsoft.VC.14.44.17.14.CRT.Headers.base.vsix",
        sha256: "852382a9aa73502b7849c1bcadfb603ba7175c4e8b60e6aba03c7de711d4ece5",
    },
    // CRT import libs (x64 Desktop)
    Payload {
        file_name: "Microsoft.VC.14.44.17.14.CRT.x64.Desktop.base.vsix",
        url: "https://download.visualstudio.microsoft.com/download/pr/67cf767c-5e71-47c2-a54a-cd5631e28942/f01f701a7bcd9587a340898c851424f6a52bb913a70c185ff0d5bf0288c5831a/Microsoft.VC.14.44.17.14.CRT.x64.Desktop.base.vsix",
        sha256: "f01f701a7bcd9587a340898c851424f6a52bb913a70c185ff0d5bf0288c5831a",
    },
];

/// SDK headers (windows.h, ...)
const SDK_HEADERS: MsiComponent = MsiComponent {
    label: "Windows SDK Desktop Headers x64",
    msi: Payload {
        file_name: "Windows SDK Desktop Headers x64-x86_en-us.msi",
        url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/ed222baa6d1d1dc09fb45a1827e7892a/windows%20sdk%20desktop%20headers%20x64-x86_en-us.msi",
        sha256: "D189CA50E5632B546795922E2262794C068D6FF301860FEB4522B8B93CBB3BA8",
    },
    cabs: &[Payload {
        file_name: "d1de88680a8e53fe75e01e94dc0ed767.cab",
        url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/f5ae8b50cc21a7ed5bcace1a38fe8fa3/d1de88680a8e53fe75e01e94dc0ed767.cab",
        sha256: "9D88FA269DC02FD3FDE50A056C04D6DFCC5B8A15739AE0F3E7AC51CC1C88F5B8",
    }],
};

/// SDK import libs (kernel32.lib, user32.lib, ...)
const SDK_LIBS: MsiComponent = MsiComponent {
    label: "Windows SDK Desktop Libs x64",
    msi: Payload {
        file_name: "Windows SDK Desktop Libs x64-x86_en-us.msi",
        url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/a7dce16da158fe456395566e2dafd23d/windows%20sdk%20desktop%20libs%20x64-x86_en-us.msi",
        sha256: "2E956CA1CF17800B0F9811A6249B945F045ADDF18EE85FFE7AEA99EC6C27243A",
    },
    cabs: &[Payload {
        file_name: "58314d0646d7e1a25e97c902166c3155.cab",
        url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/cb954f8bc3015e25cfd985a5fff3452a/58314d0646d7e1a25e97c902166c3155.cab",
        sha256: "EC209A224C9B2D31F3409208D30F5A6335C55217AD384D6212C833AC83360EBA",
    }],
};

/// rc.exe, mt.exe, midl.exe, ...
const SDK_TOOLS: MsiComponent = MsiComponent {
    label: "Windows SDK Desktop Tools x64",
    msi: Payload {
        file_name: "Windows SDK Desktop Tools x64-x86_en-us.msi",
        url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/2509ce9c6746f0629e5a4905b022be80/windows%20sdk%20desktop%20tools%20x64-x86_en-us.msi",
        sha256: "5AF8B39E5B8E40C7235B447E7D18CE4607209734F3C506006FADCDEB8931C136",
    },
    cabs: &[
        Payload {
            file_name: "cdea5502a35d09ddfbcda12e3a391dc0.cab",
            url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/b2d1f784d9f524b43e107fcb420e7cad/cdea5502a35d09ddfbcda12e3a391dc0.cab",
            sha256: "76A16062CC9764CCEB9F0A4E1F43FDEA97AFFA70752C83542562FC1F30FB9E60",
        },
        Payload {
            file_name: "19248fabbb2098a7b88c4a2786066bcc.cab",
            url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/2e009aabfde2988589258b1c79f89411/19248fabbb2098a7b88c4a2786066bcc.cab",
            sha256: "6CCBD0B699534B8CC24784E9FBBF242196332053313075C334C126E90C8A21E7",
        },
    ],
};

/// ucrt headers + import libs (stdio.h, ucrt.lib, ...)
const SDK_UCRT: MsiComponent = MsiComponent {
    label: "Universal CRT Headers Libraries and Sources",
    msi: Payload {
        file_name: "Universal CRT Headers Libraries and Sources-x86_en-us.msi",
        url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/d10da41a6ad6809f823ef4a92d4f6c56/universal%20crt%20headers%20libraries%20and%20sources-x86_en-us.msi",
        sha256: "F611CE8A9E576E3383917B04B6FBE5EE6BED8363C1A2A8E9D6F8335CBB422675",
    },
    cabs: &[
        Payload {
            file_name: "a1e2a83aa8a71c48c742eeaff6e71928.cab",
            url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/87fede232add653343acc94dbdac4118/a1e2a83aa8a71c48c742eeaff6e71928.cab",
            sha256: "29F8ED0537B49087321DFB7CCE60AAD7252900ECFBD81D6336FDB67056778A5D",
        },
        Payload {
            file_name: "f9ff50431335056fb4fbac05b8268204.cab",
            url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/8383be7caac218b9afd6a3564dbb0984/f9ff50431335056fb4fbac05b8268204.cab",
            sha256: "355CC1E65B9E5F02A0B3A4F32D02F9241B97030D3527166EFF6A372D5D0E1BAC",
        },
        Payload {
            file_name: "d95da93904819b1f7e68adb98b49a9c7.cab",
            url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/8528492e1ce2a653db74d3988d9ee96b/d95da93904819b1f7e68adb98b49a9c7.cab",
            sha256: "BC3BEABEBC0A9F161BBBE69DBCE0075019CA6E40F5DF5A8B2342A8A2AB25B22A",
        },
        Payload {
            file_name: "beb5360d2daaa3167dea7ad16c28f996.cab",
            url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/b0082c046bf17896e9730ca9f40200ac/beb5360d2daaa3167dea7ad16c28f996.cab",
            sha256: "6FEAABF4B1B09B4E3210ADDDB12C8C8D6702D731DA033784EF0330488F5BEF51",
        },
        Payload {
            file_name: "16ab2ea2187acffa6435e334796c8c89.cab",
            url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/d56a87b40b1de33c2c39a1a3d009e148/16ab2ea2187acffa6435e334796c8c89.cab",
            sha256: "D29E10BB5CE2E28957B5635B6EEB6A491FDB311C925B398443451F953F399BC2",
        },
        Payload {
            file_name: "7afc7b670accd8e3cc94cfffd516f5cb.cab",
            url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/fdde52f2c4a6db47e015e514a79c3454/7afc7b670accd8e3cc94cfffd516f5cb.cab",
            sha256: "1D99DC10063C05E8B34B82AF18DB61C080809456D471674D0272F071526DF0AB",
        },
        Payload {
            file_name: "6ee7bbee8435130a869cf971694fd9e2.cab",
            url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/9a7bacb65de148f099902218ada3394b/6ee7bbee8435130a869cf971694fd9e2.cab",
            sha256: "04728E326214D8960A188614995B65A3E9E33F93EAF13DD3CA16FE513CDFF0DE",
        },
        Payload {
            file_name: "b2f03f34ff83ec013b9e45c7cd8e8a73.cab",
            url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/be7bcaf329bbeef873a874aee49456b7/b2f03f34ff83ec013b9e45c7cd8e8a73.cab",
            sha256: "A17B9674B79AC4C8D9C4516C41D6F32FCDE041BDB07EC7F0758C16EE8A62ECAC",
        },
        Payload {
            file_name: "eca0aa33de85194cd50ed6e0aae0156f.cab",
            url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/827f1b56c1f9090dca62cf5bef23d094/eca0aa33de85194cd50ed6e0aae0156f.cab",
            sha256: "C0C6CC329D2BE2DDBA902649C46EFB9064186C2185445451602C90D9C7EB3DD8",
        },
        Payload {
            file_name: "96076045170fe5db6d5dcf14b6f6688e.cab",
            url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/510c03213f78beff83c9149c96da2ab6/96076045170fe5db6d5dcf14b6f6688e.cab",
            sha256: "82D970F5B628250EF72467D0826260C6A9F32252F42DAA3C31FED2A23170170E",
        },
        Payload {
            file_name: "78fa3c824c2c48bd4a49ab5969adaaf7.cab",
            url: "https://download.visualstudio.microsoft.com/download/pr/6452c1f1-dc1e-413c-8b19-991b61870a8b/721e7f21ddaab126788f6f8b5c3725b4/78fa3c824c2c48bd4a49ab5969adaaf7.cab",
            sha256: "6F9096BC7C182383C22A947D1B2C994D78D1742CA25163FAD8FD8C2C848419C5",
        },
    ],
};

/// Download every payload for one MSI component into `<downloads>/<label>/`
/// (msi and cabs sitting flat in that same directory), verifying each
/// against its pinned SHA-256.
///
/// Reported live: `msiexec /a` failed every attempt with `Error 1311.
/// Source file not found (cabinet): <dir>\<hash>.cab`, per its own verbose
/// log (`/lv`) -- for this MSI's `/a` admin install, the Media table's
/// external cabinet is resolved directly beside the `.msi`, not under an
/// `Installers\` subfolder as this function used to place it (a stale
/// assumption this vendor was never actually exercised against, since
/// nothing here can build until `MsvcBuildTools` itself installs
/// successfully once).
fn fetch_msi_component(
    http: &dyn Http,
    downloads: &Path,
    component: &MsiComponent,
) -> Option<PathBuf> {
    let dir = downloads.join(sanitize(component.label));
    if fs::create_dir_all(&dir).is_err() {
        logger::failure(&format!(
            "    Could not create download dir for {}",
            component.label
        ));
        return None;
    }

    let msi_path = dir.join(component.msi.file_name);
    if !fetch_and_verify(http, &component.msi, &msi_path) {
        return None;
    }
    for cab in component.cabs {
        let cab_path = dir.join(cab.file_name);
        if !fetch_and_verify(http, cab, &cab_path) {
            return None;
        }
    }
    Some(msi_path)
}

/// Download one payload to `dest` (skipped if it already exists with the
/// right hash -- a retried install must not re-fetch ~250 MB), then verify
/// its pinned SHA-256. A mismatch is always fatal here: unlike
/// vendors.json's optional `checksum` field, every digest below came from
/// Microsoft's own manifest for this exact file.
fn fetch_and_verify(http: &dyn Http, payload: &Payload, dest: &Path) -> bool {
    let info = ChecksumInfo {
        algorithm: "SHA256".into(),
        value: payload.sha256.into(),
        required: true,
    };

    if dest.is_file() && checksum::verify(dest, &info).success {
        return true;
    }

    logger::status(&format!("    Downloading {}...", payload.file_name));
    if !http.download(payload.url, dest) {
        logger::failure(&format!("    Failed to download {}", payload.file_name));
        return false;
    }
    let result = checksum::verify(dest, &info);
    if !result.success {
        logger::failure(&format!(
            "    Checksum mismatch for {}: expected {}, got {}",
            payload.file_name,
            result.expected.unwrap_or_default(),
            result.actual.unwrap_or_default()
        ));
        let _ = fs::remove_file(dest);
        return false;
    }
    true
}

/// `msiexec /a "<msi>" /qn KITSROOT="<merged>\Windows Kits\10\\" TARGETDIR="<scratch>"`.
///
/// `KITSROOT` is the public MSI property every Windows SDK Desktop/UCRT
/// package roots its Include/Lib/bin Directory-table entries under (visible
/// in the Burn manifest as `<MsiProperty Id="KITSROOT" Value="[KITSROOT]" />`
/// -- Burn forwards its own resolved value through unchanged). Passing it
/// explicitly redirects the SDK's own install layout directly into the
/// merged tree without needing a per-package hoist.
///
/// `KITSROOT` always contains a space (`Windows Kits`), which forces
/// `Command::arg`'s automatic Windows quoting to wrap the *entire*
/// `KITSROOT=...` token in one outer pair of quotes. Reported live:
/// msiexec then aborted every attempt with ERROR_INVALID_COMMAND_LINE
/// (1639) before writing a single log line -- unlike every other Windows
/// program, msiexec's own command-line parser only accepts a quoted
/// *value* half (`PROPERTY="value"`), not a quoted whole token
/// (`"PROPERTY=value"`). `Command::arg` has no way to express that split
/// (it quotes, or doesn't, the whole argument it's given), so
/// `push_msi_property` appends the raw text itself via `raw_arg`, quoting
/// only the value and doubling a trailing backslash the same way
/// `Command::arg`'s own escaping would have.
///
/// Defensive because this cannot be exercised against every SDK release:
/// if nothing landed directly under `sdk_root` (`KITSROOT` not honoured, or
/// nested one level deeper than expected), search `scratch` for the same
/// marker and merge it up instead of silently reporting success on an
/// empty tree.
fn extract_msi_component(msi_path: &Path, sdk_root: &Path, marker: &str) -> bool {
    let scratch = msi_path
        .parent()
        .unwrap_or(msi_path)
        .join("__extract_scratch");
    let _ = fs::remove_dir_all(&scratch);
    if fs::create_dir_all(&scratch).is_err() {
        return false;
    }

    let mut command = Command::new("msiexec.exe");
    command.arg("/a").arg(msi_path).arg("/qn");
    push_msi_property(&mut command, "KITSROOT", &format!("{}\\", sdk_root.display()));
    push_msi_property(&mut command, "TARGETDIR", &scratch.display().to_string());
    let status = command.status();
    let ran = matches!(status, Ok(s) if s.success());
    if !ran {
        logger::failure(&format!(
            "    msiexec extraction failed for {}",
            msi_path.display()
        ));
        let _ = fs::remove_dir_all(&scratch);
        return false;
    }

    let ok = if join_backslash_path(sdk_root, marker).is_dir() {
        true
    } else if let Some(found) = find_dir_named(&scratch, marker) {
        let dest = join_backslash_path(sdk_root, marker);
        merge_tree(&found, &dest).is_ok()
    } else {
        logger::failure(&format!(
            "    {} did not produce the expected {marker}",
            msi_path.display()
        ));
        false
    };
    let _ = fs::remove_dir_all(&scratch);
    ok
}

/// Join a `\`-separated relative path (e.g. `Include\10.0.26100.0\ucrt`)
/// onto `base`, one component at a time -- `\` is only a path separator on
/// Windows, and this runs in the crate's cross-platform test suite too
/// (`ci.yml` also tests on `ubuntu-latest`), where a single `Path::join`
/// call with an embedded `\` produces one component literally named
/// `Include\10.0.26100.0\ucrt` instead of three nested ones.
fn join_backslash_path(base: &Path, marker: &str) -> PathBuf {
    marker
        .split('\\')
        .fold(base.to_path_buf(), |acc, part| acc.join(part))
}

/// Depth-first search for a directory ending in `marker` (see
/// [`join_backslash_path`]).
fn find_dir_named(root: &Path, marker: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let candidate = join_backslash_path(&dir, marker);
        if candidate.is_dir() {
            return Some(candidate);
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                stack.push(entry.path());
            }
        }
    }
    None
}

/// Recursive copy of `src` onto `dst`, creating directories as needed and
/// overwriting existing files -- a union merge, since VC++ Tools packages
/// contribute non-overlapping subdirectories of the same
/// `VC\Tools\MSVC\<ver>\` tree.
fn merge_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dest_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            merge_tree(&entry.path(), &dest_path)?;
        } else {
            if dest_path.is_file() {
                let _ = fs::remove_file(&dest_path);
            }
            fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// One VC++ Tools VSIX: download+verify, extract as a plain zip (VSIX *is*
/// zip; the `.vsix` extension just is not one `archives::extract_archive`
/// dispatches on), then merge its `Contents\VC\Tools\MSVC\<ver>\`
/// subtree onto the shared target.
fn fetch_and_merge_vc_package(
    http: &dyn Http,
    downloads: &Path,
    target: &Path,
    payload: &Payload,
) -> bool {
    let archive_path = downloads.join(payload.file_name);
    if !fetch_and_verify(http, payload, &archive_path) {
        return false;
    }

    let staging = downloads.join(format!("{}.extracted", payload.file_name));
    let _ = fs::remove_dir_all(&staging);
    if crate::archives::extract_zip_plain(&archive_path, &staging).is_err() {
        logger::failure(&format!("    Failed to extract {}", payload.file_name));
        return false;
    }

    let contents = staging
        .join("Contents")
        .join("VC")
        .join("Tools")
        .join("MSVC")
        .join(MSVC_TOOLSET_VERSION);
    if !contents.is_dir() {
        logger::failure(&format!(
            "    {} did not contain the expected Contents\\VC\\Tools\\MSVC\\{} layout",
            payload.file_name, MSVC_TOOLSET_VERSION
        ));
        let _ = fs::remove_dir_all(&staging);
        return false;
    }
    let dest = target
        .join("VC")
        .join("Tools")
        .join("MSVC")
        .join(MSVC_TOOLSET_VERSION);
    let merged = merge_tree(&contents, &dest).is_ok();
    let _ = fs::remove_dir_all(&staging);
    merged
}

/// Install the pinned VC++ Tools compiler/linker/CRT and Windows SDK
/// headers/libs/tools into `target` (the vendor's own directory), without
/// running any installer that requires admin. Called directly from
/// `installer::UnifiedVendorInstaller::install_vendor_inner` for the
/// `MsvcBuildTools` vendor key, bypassing the generic single-artifact
/// resolve/download/extract pipeline entirely -- this vendor's shape (many
/// payloads merged into one tree) does not fit it.
pub(crate) fn install(http: &dyn Http, downloads: &Path, target: &Path) -> bool {
    logger::status("  Fetching VC++ Tools (~80 MB)...");
    for payload in VC_PACKAGES {
        if !fetch_and_merge_vc_package(http, downloads, target, payload) {
            return false;
        }
    }

    logger::status("  Fetching Windows SDK (~150 MB)...");
    let sdk_root = target.join("Windows Kits").join("10");
    let markers: Vec<(&MsiComponent, String)> = vec![
        (&SDK_HEADERS, format!("Include\\{SDK_VERSION}\\um")),
        (&SDK_LIBS, format!("Lib\\{SDK_VERSION}\\um\\x64")),
        (&SDK_TOOLS, format!("bin\\{SDK_VERSION}\\x64")),
        (&SDK_UCRT, format!("Include\\{SDK_VERSION}\\ucrt")),
    ];
    for (component, marker) in markers {
        logger::status(&format!("    {}...", component.label));
        let Some(msi_path) = fetch_msi_component(http, downloads, component) else {
            return false;
        };
        if !extract_msi_component(&msi_path, &sdk_root, &marker) {
            return false;
        }
    }

    true
}

/// A component label as a filesystem-safe directory name.
fn sanitize(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_sha256(value: &str) -> bool {
        value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
    }

    #[test]
    fn every_vc_package_has_a_valid_pinned_sha256() {
        assert_eq!(VC_PACKAGES.len(), 4);
        for package in VC_PACKAGES {
            assert!(
                is_sha256(package.sha256),
                "{} has a malformed sha256: {}",
                package.file_name,
                package.sha256
            );
            assert!(package.file_name.ends_with(".vsix"));
        }
    }

    #[test]
    fn every_sdk_component_and_its_cabs_have_valid_pinned_sha256() {
        for component in [&SDK_HEADERS, &SDK_LIBS, &SDK_TOOLS, &SDK_UCRT] {
            assert!(
                is_sha256(component.msi.sha256),
                "{} msi has a malformed sha256",
                component.label
            );
            assert!(component.msi.file_name.ends_with(".msi"));
            assert!(
                !component.cabs.is_empty(),
                "{} has no external cabs",
                component.label
            );
            for cab in component.cabs {
                assert!(
                    is_sha256(cab.sha256),
                    "{} cab {} has a malformed sha256",
                    component.label,
                    cab.file_name
                );
                assert!(cab.file_name.ends_with(".cab"));
            }
        }
    }

    #[test]
    fn sanitize_replaces_non_alphanumeric_characters() {
        assert_eq!(
            sanitize("Windows SDK Desktop Headers x64"),
            "Windows_SDK_Desktop_Headers_x64"
        );
        assert_eq!(
            sanitize("Universal CRT Headers, Libraries and Sources"),
            "Universal_CRT_Headers__Libraries_and_Sources"
        );
    }

    #[test]
    fn find_dir_named_locates_a_nested_marker() {
        let root = tempfile::tempdir().unwrap();
        let nested = root
            .path()
            .join("a")
            .join("b")
            .join("Include")
            .join("10.0.26100.0")
            .join("ucrt");
        std::fs::create_dir_all(&nested).unwrap();

        let found = find_dir_named(root.path(), "Include\\10.0.26100.0\\ucrt");
        assert_eq!(found.as_deref(), Some(nested.as_path()));
    }

    #[test]
    fn find_dir_named_returns_none_when_absent() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("a")).unwrap();
        assert!(find_dir_named(root.path(), "Include\\10.0.26100.0\\ucrt").is_none());
    }

    #[test]
    fn merge_tree_copies_nested_files_and_preserves_unrelated_existing_ones() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        let dst = root.path().join("dst");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("top.txt"), "top").unwrap();
        std::fs::write(src.join("sub").join("nested.txt"), "nested").unwrap();

        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(dst.join("existing.txt"), "keep me").unwrap();
        std::fs::write(dst.join("top.txt"), "stale").unwrap();

        merge_tree(&src, &dst).unwrap();

        assert_eq!(std::fs::read_to_string(dst.join("top.txt")).unwrap(), "top");
        assert_eq!(
            std::fs::read_to_string(dst.join("sub").join("nested.txt")).unwrap(),
            "nested"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("existing.txt")).unwrap(),
            "keep me"
        );
    }

    #[test]
    fn msvc_toolset_and_sdk_version_constants_are_nonempty() {
        assert!(!MSVC_TOOLSET_VERSION.is_empty());
        assert!(!SDK_VERSION.is_empty());
    }

    /// The shipped vendor JSON's `pathPrecedence`/`environmentVariables`
    /// hardcode `MSVC_TOOLSET_VERSION`/`SDK_VERSION` as literal path
    /// segments (JSON cannot reference a Rust const) -- this is what
    /// catches the two silently drifting apart on a future pin refresh.
    #[test]
    fn shipped_vendor_json_paths_match_the_pinned_version_constants() {
        const SHIPPED: &str =
            include_str!("../../../../dist-assets/config/vendors/MsvcBuildTools.json");
        assert!(
            SHIPPED.contains(MSVC_TOOLSET_VERSION),
            "MsvcBuildTools.json does not mention MSVC_TOOLSET_VERSION {MSVC_TOOLSET_VERSION}"
        );
        assert!(
            SHIPPED.contains(SDK_VERSION),
            "MsvcBuildTools.json does not mention SDK_VERSION {SDK_VERSION}"
        );
    }
}
