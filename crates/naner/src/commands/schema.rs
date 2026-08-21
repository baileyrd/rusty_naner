//! Command: `naner schema [config|vendors]`
//! Generates JSON Schema definitions for naner.json and vendors.json
//! configuration files to enable IDE auto-completion and validation.

use serde_json::json;

pub fn execute(args: &[String]) -> i32 {
    let target = args
        .first()
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "config".to_string());

    match target.as_str() {
        "config" => {
            let schema = config_schema_value();
            println!("{}", serde_json::to_string_pretty(&schema).unwrap());
            0
        }
        "vendors" => {
            let schema = vendors_schema_value();
            println!("{}", serde_json::to_string_pretty(&schema).unwrap());
            0
        }
        other => {
            eprintln!("Unknown schema target '{other}'. Valid targets: 'config', 'vendors'");
            1
        }
    }
}

/// The `naner.json` schema. A function rather than an inline literal so the
/// drift tests below check the exact value the command prints.
fn config_schema_value() -> serde_json::Value {
    json!({
                "$schema": "http://json-schema.org/draft-07/schema#",
                "title": "NanerConfig",
                "description": "Configuration schema for naner terminal launcher",
                "type": "object",
                "properties": {
                    "VendorPaths": {
                        "type": "object",
                        "additionalProperties": { "type": "string" },
                        "description": "Mapping of vendor executable keys to paths"
                    },
                    "Environment": {
                        "type": "object",
                        "properties": {
                            "UnifiedPath": { "type": "boolean" },
                            "PathPrecedence": {
                                "type": "array",
                                "items": { "type": "string" }
                            },
                            "EnvironmentVariables": {
                                "type": "object",
                                "additionalProperties": { "type": "string" }
                            }
                        }
                    },
                    "DefaultProfile": { "type": "string" },
                    "Profiles": {
                        "type": "object",
                        "additionalProperties": {
                            "type": "object",
                            "properties": {
                                "Name": { "type": "string" },
                                "Description": { "type": "string" },
                                "Shell": { "type": "string" },
                                "StartingDirectory": { "type": "string" },
                                "Icon": { "type": "string" },
                                "ColorScheme": { "type": "string" },
                                "Terminal": { "type": "string" },
                                "UseVendorPath": { "type": "boolean" },
                                "PreLaunch": {
                                    "type": "string",
                                    "description": "Script run before the terminal is spawned"
                                },
                                "PostLaunch": {
                                    "type": "string",
                                    "description": "Script run after the terminal starts"
                                },
                                "CustomShell": {
                                    "type": "object",
                                    "properties": {
                                        "ExecutablePath": { "type": "string" },
                                        "Arguments": { "type": "string" }
                                    }
                                }
                            }
                        }
                    },
                    "WindowsTerminal": {
                        "type": "object",
                        "properties": {
                            "DefaultTerminal": { "type": "boolean" },
                            "LaunchMode": { "type": "string" },
                            "TabTitle": { "type": "string" },
                            "SuppressApplicationTitle": { "type": "boolean" }
                        }
                    },
                    "Advanced": {
                        "type": "object",
                        "properties": {
                            "PreservePath": { "type": "boolean" },
                            "InheritSystemPath": { "type": "boolean" },
                            "VerboseLogging": { "type": "boolean" },
                            "DebugMode": { "type": "boolean" },
                            "IsolateEnvironment": { "type": "boolean" },
                            "HomeJunctions": {
                                "type": "object",
                                "additionalProperties": { "type": "string" }
                            }
                        }
                    },
                    "CustomProfiles": {
                        "type": "object",
                        "description": "Additional profiles, same shape as Profiles",
                        "additionalProperties": { "$ref": "#/properties/Profiles/additionalProperties" }
                    }
                }
    })
}

/// The schema for one vendor definition file under `config/vendors/`.
///
/// Each file holds exactly one top-level key -- the vendor key -- so the
/// schema is a single-property object, not the `{"vendors": {...}}` root the
/// pre-split `vendors.json` used.
fn vendors_schema_value() -> serde_json::Value {
    json!({
                "$schema": "http://json-schema.org/draft-07/schema#",
                "title": "VendorDefinition",
                "description": "Configuration schema for a single naner vendor definition file",
                "type": "object",
                "minProperties": 1,
                "maxProperties": 1,
                "additionalProperties": false,
                "patternProperties": {
                    "^[A-Z][A-Za-z0-9]+$": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "description": { "type": "string" },
                                "extractDir": { "type": "string" },
                                "enabled": { "type": "boolean" },
                                "required": { "type": "boolean" },
                                "dependencies": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                },
                                "provides": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Executable names this vendor puts on PATH, for `naner suggest`"
                                },
                                "installType": { "type": "string", "enum": ["archive", "installer", "binary"] },
                                "binaryName": { "type": "string" },
                                "installerArgs": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                },
                                "checksumSource": {
                                    "type": "object",
                                    "description": "Where to fetch a digest for a dynamically-resolved artifact",
                                    "properties": {
                                        "type": { "type": "string", "enum": ["sidecar", "scrape"] },
                                        "suffix": { "type": "string" },
                                        "url": { "type": "string" },
                                        "pattern": { "type": "string" }
                                    }
                                },
                                "releaseSource": {
                                    "type": "object",
                                    "properties": {
                                        "type": {
                                            "type": "string",
                                            "enum": [
                                                "github",
                                                "web-scrape",
                                                "static",
                                                "golang-api",
                                                "nodejs-api",
                                                "dotnet-api",
                                                "npm",
                                                "pip"
                                            ]
                                        },
                                        "repo": { "type": "string" },
                                        "assetPattern": { "type": "string" },
                                        "package": { "type": "string" },
                                        "url": { "type": "string" },
                                        "pattern": { "type": "string" }
                                    }
                                },
                                "checksum": {
                                    "type": "object",
                                    "properties": {
                                        "algorithm": { "type": "string" },
                                        "value": { "type": "string" },
                                        "required": { "type": "boolean" }
                                    }
                                },
                                "pathPriority": { "type": "integer" },
                                "pathPrecedence": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                },
                                "environmentVariables": {
                                    "type": "object",
                                    "additionalProperties": { "type": "string" }
                                }
                            }
                        }
                    }
    })
}

#[cfg(test)]
mod tests {
    use naner_core::config::{NanerConfig, ProfileConfig};

    /// Every key serde actually emits must be described by the schema.
    ///
    /// The schema is a hand-written literal — a third description of the config
    /// format alongside the serde structs and the shipped `*-schema.json`. This
    /// test makes serde the source of truth so the literal cannot drift again,
    /// which is how it came to advertise a `Services` block that no field
    /// backed and to omit `Advanced`, `WindowsTerminal` and `CustomProfiles`.
    #[test]
    fn schema_describes_every_field_serde_emits() {
        let mut config = NanerConfig::default();
        // Force the profile shape to serialize by giving it one entry.
        config
            .profiles
            .insert("Sample".into(), ProfileConfig::default());

        let emitted: serde_json::Value = serde_json::to_value(&config).unwrap();
        let schema = super::config_schema_value();

        let top: Vec<String> = emitted.as_object().unwrap().keys().cloned().collect();
        let described = schema["properties"].as_object().unwrap();
        for key in &top {
            assert!(
                described.contains_key(key),
                "schema omits top-level field {key:?} that serde emits"
            );
        }

        let profile_emitted = &emitted["Profiles"]["Sample"];
        let profile_described =
            schema["properties"]["Profiles"]["additionalProperties"]["properties"]
                .as_object()
                .unwrap();
        for key in profile_emitted.as_object().unwrap().keys() {
            assert!(
                profile_described.contains_key(key),
                "schema omits profile field {key:?} that serde emits"
            );
        }
    }

    /// The inverse: the schema must not invent fields the model has no home
    /// for. `Services` ("background sidecar daemons") was described in detail
    /// and backed by nothing, so a user following the schema got a silently
    /// ignored config block.
    #[test]
    fn schema_does_not_describe_fields_the_model_lacks() {
        let mut config = NanerConfig::default();
        config
            .profiles
            .insert("Sample".into(), ProfileConfig::default());
        let emitted = serde_json::to_value(&config).unwrap();
        let emitted_keys: Vec<&String> = emitted.as_object().unwrap().keys().collect();

        let schema = super::config_schema_value();
        for key in schema["properties"].as_object().unwrap().keys() {
            assert!(
                emitted_keys.contains(&key),
                "schema describes {key:?}, which no config field produces"
            );
        }
    }
}
