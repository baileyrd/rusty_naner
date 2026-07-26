//! Command: `naner schema [config|vendors]`
//! Generates JSON Schema definitions for naner.json and vendors.json
//! configuration files to enable IDE auto-completion and validation.

use serde_json::json;

pub fn execute(args: &[String]) -> i32 {
    let target = args.first().map(|s| s.to_lowercase()).unwrap_or_else(|| "config".to_string());

    match target.as_str() {
        "config" => {
            let schema = json!({
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
                    "Services": {
                        "type": "array",
                        "description": "Background sidecar daemons to run alongside terminal sessions",
                        "items": {
                            "type": "object",
                            "properties": {
                                "Name": { "type": "string" },
                                "Command": { "type": "string" },
                                "Arguments": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                },
                                "AutoRestart": { "type": "boolean" }
                            },
                            "required": ["Name", "Command"]
                        }
                    }
                }
            });
            println!("{}", serde_json::to_string_pretty(&schema).unwrap());
            0
        }
        "vendors" => {
            let schema = json!({
                "$schema": "http://json-schema.org/draft-07/schema#",
                "title": "VendorsConfig",
                "description": "Configuration schema for naner vendors manifest",
                "type": "object",
                "properties": {
                    "vendors": {
                        "type": "object",
                        "additionalProperties": {
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
                                "releaseSource": {
                                    "type": "object",
                                    "properties": {
                                        "type": { "type": "string" },
                                        "repo": { "type": "string" },
                                        "assetPattern": { "type": "string" },
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
                                }
                            }
                        }
                    }
                }
            });
            println!("{}", serde_json::to_string_pretty(&schema).unwrap());
            0
        }
        other => {
            eprintln!("Unknown schema target '{other}'. Valid targets: 'config', 'vendors'");
            1
        }
    }
}
