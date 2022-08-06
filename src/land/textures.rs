use crate::ParsedPlugin;
use itertools::Itertools;
use std::collections::HashMap;
use std::default::default;
use std::sync::Arc;
use tes3::esp::{LandscapeTexture, ObjectFlags};

pub struct RemappedTextures {
    inner: HashMap<u16, u16>,
}

impl RemappedTextures {
    fn with_capacity(len: usize) -> Self {
        assert!(len < 65535, "exceeded 65535 textures");
        Self {
            inner: HashMap::with_capacity(len),
        }
    }

    pub fn new(known_textures: &KnownTextures) -> Self {
        Self::with_capacity(known_textures.len())
    }

    pub fn from(used_ids: &[bool]) -> Self {
        let mut new = Self::with_capacity(used_ids.len());

        for (new_id, (idx, _)) in used_ids
            .iter()
            .enumerate()
            .filter(|(_, is_used)| **is_used)
            .enumerate()
        {
            new.inner.insert(
                idx.try_into().expect("safe"),
                new_id.try_into().expect("safe"),
            );
        }

        new
    }

    pub fn try_remapped_index(&self, index: u16) -> Option<u16> {
        let key = index;
        if key == 0 {
            // Default texture index.
            Some(0)
        } else {
            let old_index = key - 1;
            self.inner.get(&old_index).map(|index| *index + 1)
        }
    }

    pub fn remapped_index(&self, index: u16) -> u16 {
        self.try_remapped_index(index)
            .expect("missing remapped texture index")
    }
}

pub struct KnownTexture {
    inner: LandscapeTexture,
    pub plugin: Arc<ParsedPlugin>,
}

impl KnownTexture {
    pub fn id(&self) -> &String {
        &self.inner.id
    }

    pub fn index(&self) -> u16 {
        texture_index(&self.inner)
    }

    pub fn clone_landscape_texture(&self) -> LandscapeTexture {
        self.inner.clone()
    }
}

pub struct KnownTextures {
    inner: HashMap<String, KnownTexture>,
}

fn texture_index(texture: &LandscapeTexture) -> u16 {
    texture
        .index
        .expect("missing texture index")
        .try_into()
        .expect("invalid texture index")
}

impl KnownTextures {
    pub fn new() -> KnownTextures {
        Self { inner: default() }
    }

    pub fn sorted(&self) -> impl Iterator<Item = &KnownTexture> + '_ {
        self.inner
            .values()
            .sorted_by(|a, b| a.index().cmp(&b.index()))
    }

    pub fn update_texture(&mut self, plugin: &Arc<ParsedPlugin>, texture: &LandscapeTexture) {
        let known_texture = self.inner.get_mut(&texture.id).expect("unknown texture ID");
        if let Some(texture) = &texture.texture {
            known_texture.inner.texture = Some(texture.into());
            known_texture.plugin = plugin.clone();
        }
    }

    pub fn add_texture(
        &mut self,
        plugin: &Arc<ParsedPlugin>,
        texture: &LandscapeTexture,
    ) -> (u16, u16) {
        let old_index = texture_index(texture);

        let new_index = if self.inner.contains_key(&texture.id) {
            self.inner.get(&texture.id).expect("safe").index()
        } else {
            self.add_next_texture(plugin, texture)
        };

        (old_index, new_index)
    }

    pub fn add_remapped_texture(
        &mut self,
        plugin: &Arc<ParsedPlugin>,
        texture: &LandscapeTexture,
        remapped_textures: &mut RemappedTextures,
    ) {
        let (old_id, new_id) = self.add_texture(plugin, texture);
        remapped_textures.inner.insert(old_id, new_id);
    }

    pub fn remove_unused(&mut self, remapped_textures: &RemappedTextures) -> usize {
        let mut unused_ids = Vec::new();
        for (id, texture) in self.inner.iter_mut() {
            if let Some(new_idx) = remapped_textures.try_remapped_index(texture.index()) {
                texture.inner.index = Some(new_idx.into());
            } else {
                unused_ids.push(id.clone());
            }
        }

        let num_removed_ids = unused_ids.len();

        for id in unused_ids {
            self.inner.remove(&id);
        }

        num_removed_ids
    }

    pub fn len(&self) -> usize {
        let len = self.inner.len();
        assert!(len < 65535, "exceeded 65535 textures");
        len
    }

    fn next_texture_index(&self) -> u16 {
        self.len().try_into().expect("safe")
    }

    fn add_next_texture(&mut self, plugin: &Arc<ParsedPlugin>, texture: &LandscapeTexture) -> u16 {
        let next_index = self.next_texture_index();

        let mut inner = texture.clone();

        assert!(
            !inner.flags.contains(ObjectFlags::DELETED),
            "tried to add deleted LTEX"
        );

        inner.index = Some(next_index.into());

        let known_texture = KnownTexture {
            inner,
            plugin: plugin.clone(),
        };

        self.inner.insert(texture.id.clone(), known_texture);
        next_index
    }
}
