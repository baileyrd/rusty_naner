# Windows Terminal Configuration Template

This directory contains the default Windows Terminal configuration template used by Naner.

## Files

- **settings.json** - Template configuration with Naner profiles

## How It Works

When Naner installs Windows Terminal in portable mode:

1. The installer creates a `.portable` file in `vendor/terminal/` to enable portable mode
2. It creates `vendor/terminal/LocalState/` directory
3. It copies `settings.json` from this template directory to `vendor/terminal/LocalState/settings.json`
4. It expands `%NANER_ROOT%` placeholders to the actual Naner installation path

## Customizing

To customize the default Windows Terminal configuration:

1. Edit `settings.json` in this directory
2. Use `%NANER_ROOT%` as a placeholder for the Naner installation path
3. Reinstall Windows Terminal or manually copy to `vendor/terminal/LocalState/settings.json`

## Template Variables

- `%NANER_ROOT%` - Automatically replaced with the Naner installation path
- `%USERPROFILE%` - Preserved as-is (Windows Terminal will expand it)

## Example Profile

```json
{
    "guid": "{naner-unified}",
    "name": "Naner (Unified)",
    "commandline": "\"%NANER_ROOT%\\vendor\\powershell\\pwsh.exe\" -NoExit -NoLogo -NoProfile -Command \". '%NANER_ROOT%\\home\\.config\\powershell\\profile.ps1'\"",
    "startingDirectory": "%USERPROFILE%",
    "icon": "%NANER_ROOT%\\icons\\naner.ico",
    "colorScheme": "Campbell"
}
```

## Portable Mode

Windows Terminal's portable mode stores all settings locally in the Naner directory:
- Settings location: `vendor/terminal/LocalState/settings.json`
- State (window positions): `vendor/terminal/LocalState/state.json`

This ensures your terminal configuration travels with your Naner installation.
