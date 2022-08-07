use crate::io::meta_schema::{PluginMeta, VersionedPluginMeta};
use anyhow::{anyhow, bail, Context, Result};
use filetime::FileTime;
use itertools::Itertools;
use log::{error, trace, warn};
use owo_colors::OwoColorize;
use regex::Regex;
use std::default::default;
use std::fs;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Lines};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tes3::esp::{Cell, Header, Landscape, LandscapeTexture, Plugin, TES3Object};

/// Parse a [Plugin] named `plugin_name` from the `data_files` directory.
fn parse_records(data_files: &str, plugin_name: &str) -> Result<Plugin> {
    ParsedPlugins::check_data_files(data_files)
        .with_context(|| anyhow!("Unable to find plugin {}", plugin_name))?;

    let file_path: PathBuf = [data_files, plugin_name].iter().collect();

    let mut plugin = Plugin::new();
    plugin
        .load_path_filtered(file_path, |tag| {
            matches!(
                &tag,
                Header::TAG | LandscapeTexture::TAG | Landscape::TAG | Cell::TAG
            )
        })
        .with_context(|| anyhow!("Failed to load records from plugin {}", plugin_name))?;

    plugin.objects.retain(|object| match object {
        TES3Object::Cell(cell) => cell.is_exterior(),
        _ => true,
    });

    Ok(plugin)
}

/// Open `filename` and return an iterator for the lines in the file.
fn read_lines(filename: &Path) -> Result<Lines<BufReader<File>>> {
    let file = File::open(filename).with_context(|| {
        anyhow!(
            "Unable to open file {} for reading",
            filename.to_string_lossy()
        )
    })?;
    Ok(BufReader::new(file).lines())
}

/// Returns `true` if `path` ends with `.esm`, ignoring case.
fn is_esm(path: &str) -> bool {
    Path::new(path)
        .extension()
        .map_or(false, |ext| ext.eq_ignore_ascii_case("esm"))
}

/// Sorts `plugin_list` by using the last modified date of the files in `data_files`.
pub fn sort_plugins(data_files: &str, plugin_list: &mut [String]) -> Result<()> {
    ParsedPlugins::check_data_files(data_files)
        .with_context(|| anyhow!("Unable to sort load order with last modified date"))?;

    let order = |plugin_name: &str| {
        // Order by modified time, with ESMs given priority.
        let is_esm = is_esm(plugin_name);
        let file_path: PathBuf = [data_files, plugin_name].iter().collect();
        let last_modified_time = file_path
            .metadata()
            .map(|metadata| FileTime::from_last_modification_time(&metadata))
            .expect("file does not have a last modified time");
        (!is_esm, last_modified_time)
    };

    plugin_list.sort_by(|a, b| order(a).cmp(&order(b)));

    Ok(())
}

/// Returns a `name` describing a meta file by replacing the extension with `.mergedlands.toml`.
pub fn meta_name(name: &str) -> String {
    let file_name_without_extension = Path::new(&name).file_stem().unwrap().to_string_lossy();
    format!("{}.mergedlands.toml", file_name_without_extension)
}

/// A [ParsedPlugin] is the `name`, [Plugin] records, and any [PluginMeta] data.
pub struct ParsedPlugin {
    /// The `name` of the plugin.
    pub name: String,
    /// The parsed [Plugin] records.
    pub records: Plugin,
    /// The parsed [PluginMeta], or a default if no meta file was found.
    pub meta: PluginMeta,
}

impl ParsedPlugin {
    /// Returns an empty [ParsedPlugin] with the provided `name`.
    pub fn empty(name: &str) -> Self {
        Self {
            name: name.to_string(),
            records: Plugin::new(),
            meta: default(),
        }
    }

    /// Creates a [ParsedPlugin]. If `meta` is [None], a default [PluginMeta] is created.
    fn from(name: &str, records: Plugin, meta: Option<PluginMeta>) -> Self {
        Self {
            name: name.to_string(),
            records,
            meta: meta.unwrap_or_else(default),
        }
    }
}

impl Eq for ParsedPlugin {}

impl PartialEq<Self> for ParsedPlugin {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Hash for ParsedPlugin {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state)
    }
}

/// All [ParsedPlugin] organized by `.esm` or `.esp`.
pub struct ParsedPlugins {
    /// The ordered list of `.esm` files.
    /// These will be used for creating the reference [crate::Landmass].
    pub masters: Vec<Arc<ParsedPlugin>>,
    /// The ordered list of `.esp` files.
    /// These will be created for creating each [crate::LandmassDiff].
    pub plugins: Vec<Arc<ParsedPlugin>>,
}

/// Returns a [Vec] of plugin names by reading the `.ini` file located at
/// `path`. Each plugin name is checked for existence in `data_files`.
fn read_ini_file(data_files: &str, path: &Path) -> Result<Vec<String>> {
    ParsedPlugins::check_data_files(data_files)
        .with_context(|| anyhow!("Unable to parse plugins from ini file"))?;

    let lines = read_lines(path).with_context(|| anyhow!("Unable to read Morrowind.ini"))?;

    let mut all_plugins = Vec::new();

    const QUOTE_CHARS: [char; 2] = ['\'', '"'];
    let match_game_file = Regex::new(r#"^GameFile(\d+)=(.+)$"#).expect("safe");

    let mut is_game_files = false;
    for line in lines
        .flatten()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty() && !line.starts_with(';'))
    {
        if line == "[Game Files]" {
            is_game_files = true;
        } else if line.starts_with('[') {
            is_game_files = false;
        } else if is_game_files {
            match match_game_file.captures(&line) {
                None => {
                    warn!(
                        "{}",
                        format!("Found junk in [Game Files] section: {}", line.bold()).yellow()
                    );
                }
                Some(captures) => {
                    let plugin_name = captures
                        .get(2)
                        .expect("safe")
                        .as_str()
                        .trim_start_matches(QUOTE_CHARS)
                        .trim_end_matches(QUOTE_CHARS);

                    let file_path: PathBuf = [data_files, plugin_name].iter().collect();
                    match file_path.try_exists() {
                        Ok(true) => all_plugins.push(plugin_name.to_string()),
                        Ok(false) => error!(
                            "{} {}",
                            format!("Plugin {}", plugin_name.bold()).bright_red(),
                            format!("does not exist in `{}` directory", data_files).bright_red()
                        ),
                        Err(e) => error!(
                            "{} {}",
                            format!("Could not find plugin {}", plugin_name.bold()).bright_red(),
                            format!("due to: {:?}", e.bold()).bright_red()
                        ),
                    }
                }
            }
        }
    }

    Ok(all_plugins)
}

impl ParsedPlugins {
    /// Helper function for returning an `Err` if the `data_files` does not exist
    /// or is otherwise inaccessible.
    pub fn check_data_files(data_files: &str) -> Result<()> {
        let exists = Path::new(data_files)
            .try_exists()
            .with_context(|| anyhow!("Unable to find `{}` directory", data_files))?;

        if !exists {
            bail!("The `{}` directory does not exist", data_files);
        }

        Ok(())
    }

    /// Creates a new [ParsedPlugins] from the `data_files` directory.
    /// If `plugin_names` is [None], then the `.ini` file will be read from
    /// the parent directory above `data_files` and used for the list instead.
    pub fn new(data_files: &str, plugin_names: Option<&[&str]>) -> Result<Self> {
        ParsedPlugins::check_data_files(data_files)
            .with_context(|| anyhow!("Unable to parse plugins"))?;

        let mut all_plugins = plugin_names
            .map(|plugin_names| {
                trace!("Using {} plugins provided as arguments", plugin_names.len());

                Ok::<_, anyhow::Error>(
                    plugin_names
                        .iter()
                        .map(|plugin| plugin.to_string())
                        .collect_vec(),
                )
            })
            .unwrap_or_else(|| {
                trace!("Parsing Morrowind.ini for plugins");

                let parent_directory = Path::new(data_files).parent().with_context(|| {
                    anyhow!("Unable to find parent of `{}` directory", data_files)
                })?;

                let file_path: PathBuf = [parent_directory, Path::new("Morrowind.ini")]
                    .iter()
                    .collect();

                let plugin_names = read_ini_file(data_files, &file_path)
                    .with_context(|| anyhow!("Unable to parse plugins from Morrowind.ini"))?;

                trace!(
                    "Using {} plugins parsed from Morrowind.ini",
                    plugin_names.len()
                );

                Ok(plugin_names)
            })
            .with_context(|| anyhow!("Unable to parse plugins"))?;

        // TODO(dvd): #feature Control this via config file.
        sort_plugins(data_files, &mut all_plugins)
            .with_context(|| anyhow!("Unknown load order for plugins"))?;

        let mut masters = Vec::new();
        let mut plugins = Vec::new();

        for plugin_name in all_plugins {
            match parse_records(data_files, &plugin_name) {
                Ok(records) => {
                    let meta_name = meta_name(&plugin_name);
                    let meta_file_path: PathBuf = [data_files, &meta_name].iter().collect();

                    let data = fs::read_to_string(meta_file_path)
                        .with_context(|| anyhow!("Failed to read meta file."))
                        .and_then(|text| {
                            toml::from_str::<VersionedPluginMeta>(&text)
                                .with_context(|| anyhow!("Failed to parse meta file contents."))
                        });

                    let meta = match data {
                        Ok(VersionedPluginMeta::V0(meta)) => {
                            trace!("Parsed meta file {}", meta_name);
                            Some(meta)
                        }
                        Ok(VersionedPluginMeta::Unsupported) => {
                            error!(
                                "{}",
                                format!("Unsupported plugin meta file {}", meta_name.bold())
                                    .bright_red()
                            );
                            None
                        }
                        // TODO(dvd): #refactor Is there a TOML error we could be printing here?
                        Err(_) => None,
                    };

                    let parsed_plugin = Arc::new(ParsedPlugin::from(&plugin_name, records, meta));
                    if is_esm(&plugin_name) {
                        masters.push(parsed_plugin);
                    } else {
                        plugins.push(parsed_plugin);
                    }
                }
                Err(e) => {
                    error!(
                        "{} {}",
                        format!("Failed to parse plugin {}", plugin_name.bold()).bright_red(),
                        format!("due to: {:?}", e.bold()).bright_red()
                    );
                }
            }
        }

        Ok(Self { masters, plugins })
    }
}
