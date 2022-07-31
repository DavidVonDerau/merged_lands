use std::fs::File;
use std::io;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

fn read_lines(filename: impl AsRef<Path>) -> io::Result<io::Lines<io::BufReader<File>>> {
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

fn is_esm(path: &Path) -> bool {
    path.extension() //
        .map_or(false, |ext| ext.eq_ignore_ascii_case("esm"))
}

fn sort_plugins(plugin_list: &mut Vec<PathBuf>) {
    /// Order by modified time, with ESMs given priority.
    fn order(path: &Path) -> (bool, SystemTime) {
        let is_esm = is_esm(path);
        let mtime = path
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or_else(|_| SystemTime::now());
        (!is_esm, mtime)
    }

    plugin_list.sort_by(|a, b| order(a).cmp(&order(b)));
}

pub struct ActivePluginPaths {
    pub masters: Vec<PathBuf>,
    pub plugins: Vec<PathBuf>,
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
                        game_files.push(PathBuf::from(plugin_name));
                    } else {
                        println!("Found junk in [Game Files] section: {}", line);
                    }
                }
            }
        }

        sort_plugins(&mut game_files);

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
