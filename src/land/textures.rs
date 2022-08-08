use crate::io::parsed_plugins::ParsedPlugin;
use crate::merge::relative_to::RelativeTo;
use anyhow::{bail, Error};
use const_default::ConstDefault;
use hashbrown::HashMap;
use itertools::Itertools;
use log::trace;
use std::default::default;
use std::sync::Arc;
use tes3::esp::{LandscapeTexture, ObjectFlags};

#[derive(Eq, PartialEq, Hash, Default, Copy, Clone, Debug, Ord, PartialOrd)]
/// The index stored in the `texture_indices` [TerrainMap].
/// Can be converted to [IndexLTEX].
pub struct IndexVTEX(u16);

impl IndexVTEX {
    pub fn new(value: u16) -> Self {
        Self(value)
    }

    pub fn as_u16(&self) -> u16 {
        self.0
    }
}

impl ConstDefault for IndexVTEX {
    const DEFAULT: Self = IndexVTEX(0);
}

impl From<IndexVTEX> for f64 {
    fn from(value: IndexVTEX) -> Self {
        value.0.into()
    }
}

impl RelativeTo for IndexVTEX {
    type Delta = i32;

    fn subtract(lhs: Self, rhs: Self) -> Self::Delta {
        (lhs.0 as Self::Delta) - (rhs.0 as Self::Delta)
    }

    fn add(lhs: Self, rhs: Self::Delta) -> Self {
        Self::new(((lhs.0 as Self::Delta) + rhs) as u16)
    }
}

#[derive(Eq, PartialEq, Hash, Default, Copy, Clone, Debug, Ord, PartialOrd)]
/// The index stored in each [LandscapeTexture].
/// Can be converted to [IndexVTEX].
pub struct IndexLTEX(u16);

impl IndexLTEX {
    fn new(value: u16) -> Self {
        Self(value)
    }

    pub fn as_u16(&self) -> u16 {
        self.0
    }
}

impl From<IndexLTEX> for IndexVTEX {
    fn from(value: IndexLTEX) -> Self {
        Self::new(value.0 + 1)
    }
}

impl TryFrom<IndexVTEX> for IndexLTEX {
    type Error = Error;

    fn try_from(value: IndexVTEX) -> Result<Self, Self::Error> {
        if value.0 == 0 {
            bail!("cannot convert default texture");
        } else {
            Ok(Self::new(value.0 - 1))
        }
    }
}

/// [RemappedTextures] allows remapping terrain indices.
/// Supports up to [u16::MAX] textures.
pub struct RemappedTextures {
    inner: HashMap<IndexVTEX, IndexVTEX>,
}

impl RemappedTextures {
    fn with_capacity(len: usize) -> Self {
        assert!(len < u16::MAX as usize, "exceeded 65535 textures");
        Self {
            inner: HashMap::with_capacity(len),
        }
    }

    /// Creates a new [RemappedTextures] with size to hold [KnownTextures].
    pub fn new(known_textures: &KnownTextures) -> Self {
        Self::with_capacity(known_textures.len())
    }

    /// Creates a new [RemappedTextures] from the `used_ids`.
    pub fn from(used_ids: &[bool]) -> Self {
        let mut new = Self::with_capacity(used_ids.len());

        for (new_id, (idx, _)) in used_ids
            .iter()
            .enumerate()
            .filter(|(_, is_used)| **is_used)
            .enumerate()
        {
            new.inner.insert(
                IndexVTEX::new(idx.try_into().expect("safe")),
                IndexVTEX::new(new_id.try_into().expect("safe")),
            );
        }

        new
    }

    /// Try to remap `index`.
    pub fn try_remapped_index(&self, index: IndexVTEX) -> Option<IndexVTEX> {
        if index == IndexVTEX::default() {
            Some(index)
        } else {
            self.inner.get(&index).cloned()
        }
    }

    /// Remap `index`.
    /// Asserts if `index` is missing from the [RemappedTextures].
    pub fn remapped_index(&self, index: IndexVTEX) -> IndexVTEX {
        self.try_remapped_index(index)
            .expect("missing remapped texture index")
    }
}

/// A [LandscapeTexture] and the [ParsedPlugin] that last added or modified it.
pub struct KnownTexture {
    inner: LandscapeTexture,
    pub plugin: Arc<ParsedPlugin>,
}

impl KnownTexture {
    /// The [String] `id` of the [LandscapeTexture].
    /// This uniquely identifies the [KnownTexture] within the [KnownTextures].
    pub fn id(&self) -> &String {
        &self.inner.id
    }

    /// The [u16] `index` of the [LandscapeTexture].
    /// This uniquely identifies the texture within the `texture_indices` field of [tes3::esp::Landscape]
    /// or [crate::land::landscape_diff::LandscapeDiff].
    pub fn index(&self) -> IndexLTEX {
        texture_index(&self.inner)
    }

    /// Clones the [LandscapeTexture].
    pub fn clone_landscape_texture(&self) -> LandscapeTexture {
        self.inner.clone()
    }
}

/// [KnownTextures] stores a map of [KnownTexture] accessible by the [KnownTexture::id].
/// Supports up to [u16::MAX] textures.
pub struct KnownTextures {
    inner: HashMap<String, KnownTexture>,
}

/// Returns [u16] `index` of the [LandscapeTexture].
/// Asserts if the index cannot be found or exceeds [u16::MAX].
fn texture_index(texture: &LandscapeTexture) -> IndexLTEX {
    IndexLTEX::new(
        texture
            .index
            .expect("missing texture index")
            .try_into()
            .expect("invalid texture index"),
    )
}

impl KnownTextures {
    pub fn new() -> KnownTextures {
        Self { inner: default() }
    }

    /// Returns an [Iterator] over the [KnownTexture] sorted by [KnownTexture::index].
    pub fn sorted(&self) -> impl Iterator<Item = &KnownTexture> + '_ {
        self.inner
            .values()
            .sorted_by(|a, b| a.index().cmp(&b.index()))
    }

    /// Update the [KnownTexture] matching `texture` with changes from [ParsedPlugin] `plugin`.
    pub fn update_texture(&mut self, plugin: &Arc<ParsedPlugin>, texture: &LandscapeTexture) {
        let known_texture = self.inner.get_mut(&texture.id).expect("unknown texture ID");
        if let Some(new_texture) = &texture.texture {
            if known_texture
                .inner
                .texture
                .as_ref()
                .map(|texture| texture != new_texture)
                .unwrap_or(true)
            {
                known_texture.inner.texture = Some(new_texture.into());
                known_texture.plugin = plugin.clone();
            }
        }
    }

    /// Add a new [KnownTexture] matching `texture` from [ParsedPlugin] `plugin`.
    /// Returns a tuple corresponding to the `(old_index, new_index)`.
    fn add_texture(
        &mut self,
        plugin: &Arc<ParsedPlugin>,
        texture: &LandscapeTexture,
    ) -> (IndexLTEX, IndexLTEX) {
        let old_index = texture_index(texture);

        let new_index = if self.inner.contains_key(&texture.id) {
            self.inner.get(&texture.id).expect("safe").index()
        } else {
            self.add_next_texture(plugin, texture)
        };

        (old_index, new_index)
    }

    /// Add a new [KnownTexture] matching `texture` from [ParsedPlugin] `plugin`.
    /// The [RemappedTextures] is updated.
    pub fn add_remapped_texture(
        &mut self,
        plugin: &Arc<ParsedPlugin>,
        texture: &LandscapeTexture,
        remapped_textures: &mut RemappedTextures,
    ) {
        let (old_id, new_id) = self.add_texture(plugin, texture);
        if remapped_textures
            .inner
            .insert(old_id.into(), new_id.into())
            .is_none()
        {
            trace!(
                "Remapped {} from {} to {}",
                texture.id,
                old_id.as_u16(),
                new_id.as_u16()
            );
        }
    }

    /// Remove all textures from [KnownTextures] that are not present in the
    /// [RemappedTextures].
    pub fn remove_unused(&mut self, remapped_textures: &RemappedTextures) -> usize {
        let mut unused_ids = Vec::new();

        for (id, texture) in self.inner.iter_mut() {
            if let Some(new_idx) = remapped_textures
                .try_remapped_index(texture.index().into())
                .map(|idx| IndexLTEX::try_from(idx).expect("safe"))
            {
                trace!(
                    "Remapped {} from {} to {}",
                    id,
                    texture.index().as_u16(),
                    new_idx.as_u16()
                );
                texture.inner.index = Some(new_idx.as_u16().into());
            } else {
                unused_ids.push(id.clone());
            }
        }

        let num_removed_ids = unused_ids.len();

        for id in unused_ids {
            trace!("Removing unused texture {}", id);
            self.inner.remove(&id);
        }

        num_removed_ids
    }

    /// The number of [KnownTexture].
    pub fn len(&self) -> usize {
        let len = self.inner.len();
        assert!(len < u16::MAX as usize, "exceeded 65535 textures");
        len
    }

    /// The next [KnownTexture::index].
    fn next_texture_index(&self) -> IndexLTEX {
        IndexLTEX::new(self.len().try_into().expect("safe"))
    }

    /// Add a new [KnownTexture] matching `texture` from [ParsedPlugin] `plugin`.
    /// The new index will be set to [Self::next_texture_index].
    fn add_next_texture(
        &mut self,
        plugin: &Arc<ParsedPlugin>,
        texture: &LandscapeTexture,
    ) -> IndexLTEX {
        let next_index = self.next_texture_index();

        let mut inner = texture.clone();

        assert!(
            !inner.flags.contains(ObjectFlags::DELETED),
            "tried to add deleted LTEX"
        );

        inner.index = Some(next_index.as_u16().into());

        let known_texture = KnownTexture {
            inner,
            plugin: plugin.clone(),
        };

        self.inner.insert(texture.id.clone(), known_texture);
        next_index
    }
}
