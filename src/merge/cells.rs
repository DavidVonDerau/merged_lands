use crate::io::meta_schema::MetaType;
use crate::io::parsed_plugins::{ParsedPlugin, ParsedPlugins};
use crate::land::terrain_map::Vec2;
use hashbrown::HashMap;
use std::default::default;
use std::sync::Arc;
use tes3::esp::Cell;

pub struct ModifiedCell {
    pub inner: Cell,
    pub plugins: Vec<Arc<ParsedPlugin>>,
}

fn merge_cell_into(lhs: &mut ModifiedCell, rhs: &Cell, plugin: &Arc<ParsedPlugin>) {
    let new = &mut lhs.inner;
    let mut is_modified = false;

    if new.flags != rhs.flags {
        new.flags |= rhs.flags;
        is_modified = true;
    }

    if new.data != rhs.data {
        assert_eq!(new.data.grid, rhs.data.grid);
        new.data.flags |= rhs.data.flags;
        is_modified = true;
    }

    if !rhs.id.is_empty() && new.id != rhs.id {
        new.id = rhs.id.clone();
        is_modified = true;
    }

    if let Some(record) = new.region.as_ref() {
        new.region = Some(record.clone());
        is_modified = true;
    }

    if let Some(record) = new.map_color.as_ref() {
        new.map_color = Some(*record);
        is_modified = true;
    }

    if let Some(record) = new.water_height.as_ref() {
        new.water_height = Some(*record);
        is_modified = true;
    }

    if let Some(record) = new.atmosphere_data.as_ref() {
        new.atmosphere_data = Some(record.clone());
        is_modified = true;
    }

    if is_modified {
        lhs.plugins.push(plugin.clone());
    } else {
        *lhs.plugins.last_mut().expect("safe") = plugin.clone();
    }
}

fn merge_cells_into(cells: &mut HashMap<Vec2<i32>, ModifiedCell>, plugins: &[Arc<ParsedPlugin>]) {
    for plugin in plugins {
        if plugin.meta.meta_type == MetaType::MergedLands {
            continue;
        }

        for cell in plugin.records.objects_of_type::<Cell>() {
            let coords = Vec2::new(cell.data.grid.0, cell.data.grid.1);
            if cells.contains_key(&coords) {
                let prev_cell = cells.get_mut(&coords).expect("safe");
                merge_cell_into(prev_cell, cell, plugin);
            } else {
                let new_cell = ModifiedCell {
                    inner: Cell {
                        flags: cell.flags,
                        id: cell.id.clone(),
                        data: cell.data.clone(),
                        region: cell.region.clone(),
                        map_color: cell.map_color,
                        water_height: cell.water_height,
                        atmosphere_data: cell.atmosphere_data.clone(),
                        references: default(),
                    },
                    plugins: vec![plugin.clone()],
                };

                cells.insert(coords, new_cell);
            };
        }
    }
}

pub fn merge_cells(parsed_plugins: &ParsedPlugins) -> HashMap<Vec2<i32>, ModifiedCell> {
    let mut cells = default();

    merge_cells_into(&mut cells, &parsed_plugins.masters);
    merge_cells_into(&mut cells, &parsed_plugins.plugins);

    cells
}
