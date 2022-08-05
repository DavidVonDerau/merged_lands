use std::sync::Arc;
use std::{io, str};
use tes3::esp::{Landscape, LandscapeTexture, Plugin};

pub fn parse_records(plugin_name: &str) -> io::Result<Arc<Plugin>> {
    let mut plugin = Plugin::new();
    let file_path = format!("Data Files/{}", plugin_name);
    plugin.load_path_filtered(file_path, |tag| {
        matches!(&tag, LandscapeTexture::TAG | Landscape::TAG)
    })?;
    Ok(Arc::new(plugin))
}
