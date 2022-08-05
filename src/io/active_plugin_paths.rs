use filetime::FileTime;
use log::warn;
use std::fs::File;
use std::io;
use std::io::BufRead;
use std::path::{Path, PathBuf};

fn read_lines(filename: impl AsRef<Path>) -> io::Result<io::Lines<io::BufReader<File>>> {
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

fn is_esm(path: &str) -> bool {
    Path::new(path)
        .extension()
        .map_or(false, |ext| ext.eq_ignore_ascii_case("esm"))
}

fn sort_plugins(data_files: &str, plugin_list: &mut [String]) {
    let order = |plugin: &str| {
        // Order by modified time, with ESMs given priority.
        let is_esm = is_esm(plugin);
        let file_path: PathBuf = [data_files, plugin].iter().collect();
        let last_modified_time = file_path
            .metadata()
            .map(|metadata| FileTime::from_last_modification_time(&metadata))
            .expect("file does not have a last modified time");
        (!is_esm, last_modified_time)
    };

    plugin_list.sort_by(|a, b| order(a).cmp(&order(b)));
}

pub struct ActivePluginPaths {
    pub masters: Vec<String>,
    pub plugins: Vec<String>,
}

impl ActivePluginPaths {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let mut game_files = Vec::new();
        let mut is_game_files = false;
        if let Ok(lines) = read_lines(path) {
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
                    // TODO(dvd): Handle edge cases here like single or double quotes.
                    let is_valid_line = line.starts_with("GameFile")
                        && line.chars().filter(|c| *c == '=').count() == 1;
                    if is_valid_line {
                        let plugin_name = line.split('=').last().unwrap().to_string();
                        game_files.push(plugin_name);
                    } else {
                        warn!("Found junk in [Game Files] section: {}", line);
                    }
                }
            }
        }

        sort_plugins("Data Files", &mut game_files);

        let mut masters = Vec::new();
        let mut plugins = Vec::new();

        for game_file in game_files {
            if is_esm(&game_file) {
                masters.push(game_file);
            } else {
                plugins.push(game_file);
            }
        }

        Self { masters, plugins }
    }
}
