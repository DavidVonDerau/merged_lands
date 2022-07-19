use anymap::AnyMap;
use bitflags::bitflags;
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use static_assertions::const_assert_eq;
use std::collections::HashMap;
use std::default::default;
use std::fmt::Debug;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::str;

const DEBUG: bool = false;

pub trait Reader: BufRead + Read + Seek {}
impl Reader for BufReader<File> {}

// File is composed of 0 or more RecordHeaders. Each RecordHeader has 0 or more FieldHeaders.

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct RecordHeader {
    type_tag: [u8; 4],
    data_size: u32,
    reserved: u32,
    flags: u32,
}
const RECORD_HEADER_LEN: usize = 16;
const_assert_eq!(std::mem::size_of::<RecordHeader>(), RECORD_HEADER_LEN);

pub trait ESHeader {
    fn from_reader<T: Reader>(file: &mut T) -> Self;

    fn name(&self) -> &str;

    fn id(&self) -> u32;

    fn size(&self) -> usize;

    fn skip<T: Reader>(&self, file: &mut T);
}

impl ESHeader for RecordHeader {
    fn from_reader<T: Reader>(file: &mut T) -> Self {
        let mut type_tag = [0; 4];
        file.read_exact(&mut type_tag).unwrap();
        Self {
            type_tag,
            data_size: file.read_u32::<LittleEndian>().unwrap(),
            reserved: file.read_u32::<LittleEndian>().unwrap(),
            flags: file.read_u32::<LittleEndian>().unwrap(),
        }
    }

    fn name(&self) -> &str {
        unsafe { str::from_utf8_unchecked(&self.type_tag) }
    }

    fn id(&self) -> u32 {
        bincode::deserialize(&self.type_tag).unwrap()
    }

    fn size(&self) -> usize {
        self.data_size as usize
    }

    fn skip<T: Reader>(&self, file: &mut T) {
        file.seek(SeekFrom::Current(self.size() as i64)).unwrap();
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct FieldHeader {
    type_tag: [u8; 4],
    data_size: u32,
}
const FIELD_HEADER_LEN: usize = 8;
const_assert_eq!(std::mem::size_of::<FieldHeader>(), FIELD_HEADER_LEN);

impl ESHeader for FieldHeader {
    fn from_reader<T: Reader>(file: &mut T) -> Self {
        let mut type_tag = [0; 4];
        file.read_exact(&mut type_tag).unwrap();
        Self {
            type_tag,
            data_size: file.read_u32::<LittleEndian>().unwrap(),
        }
    }

    fn name(&self) -> &str {
        unsafe { str::from_utf8_unchecked(&self.type_tag) }
    }

    fn id(&self) -> u32 {
        bincode::deserialize(&self.type_tag).unwrap()
    }

    fn size(&self) -> usize {
        self.data_size as usize
    }

    fn skip<T: Reader>(&self, file: &mut T) {
        file.seek(SeekFrom::Current(self.size() as i64)).unwrap();
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
#[repr(C)]
pub struct Vec2<T> {
    pub x: T,
    pub y: T,
}
const I32VEC2_LEN: usize = 8;
const_assert_eq!(std::mem::size_of::<Vec2<i32>>(), I32VEC2_LEN);

impl Vec2<i32> {
    fn from_reader<T: Reader>(file: &mut T) -> Self {
        Self {
            x: file.read_i32::<LittleEndian>().unwrap(),
            y: file.read_i32::<LittleEndian>().unwrap(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, PartialEq, Default, Hash)]
#[repr(C)]
pub struct Vec3<T> {
    pub x: T,
    pub y: T,
    pub z: T,
}

const I8VEC3_LEN: usize = 3;
const_assert_eq!(std::mem::size_of::<Vec3<i8>>(), I8VEC3_LEN);

const U8VEC3_LEN: usize = 3;
const_assert_eq!(std::mem::size_of::<Vec3<u8>>(), U8VEC3_LEN);

bitflags! {
    #[derive(Default)]
    pub struct DataFlags: u32 {
        const NONE = 0b0;
        const VNML_VHGT_WNAM = 0b1;
        const VCLR = 0b10;
        const VTEX = 0b100;
        const VNML = 0b1000;
        const VHGT = 0b10000;
        const WNAM = 0b100000;
    }
}

impl DataFlags {
    fn from_reader<T: Reader>(file: &mut T) -> Self {
        let bits = file.read_u32::<LittleEndian>().unwrap();
        DataFlags::from_bits_truncate(bits)
    }
}

pub type TerrainMap<U, const T: usize> = [[U; T]; T];

#[derive(Copy, Clone)]
pub struct HeightData {
    pub offset: f32,
    pub differences: TerrainMap<i8, 65>,
    _reserved: [u8; 3],
}

impl HeightData {
    fn from_reader<T: Reader>(file: &mut T) -> Self {
        let mut differences = [[0; 65]; 65];
        let mut reserved = [0; 3];

        let offset = file.read_f32::<LittleEndian>().unwrap();
        for ii in 0..65 {
            file.read_i8_into(&mut differences[ii]).unwrap();
        }
        file.read_exact(&mut reserved).unwrap();

        Self {
            offset,
            differences,
            _reserved: reserved,
        }
    }
}

#[derive(Copy, Clone)]
pub struct LAND {
    pub coordinates: Vec2<i32>,
    pub included_data: DataFlags,
    pub vertex_normals: Option<TerrainMap<Vec3<i8>, 65>>,
    pub height_data: Option<HeightData>,
    pub world_map_height: Option<TerrainMap<u8, 9>>,
    pub vertex_colors: Option<TerrainMap<Vec3<u8>, 65>>,
    pub vertex_textures: Option<TerrainMap<u16, 16>>,
}

fn from_reader<T: Reader, F: Sized, const U: usize>(file: &mut T) -> F {
    assert_eq!(U, std::mem::size_of::<F>());
    let mut raw_bytes = [0; U];
    file.read_exact(&mut raw_bytes).unwrap();
    unsafe { std::ptr::read(raw_bytes.as_ptr() as *const _) }
}

fn from_zstring<T: Reader>(file: &mut T, size: usize) -> String {
    let mut zstring = String::with_capacity(size);
    let mut num_bytes = 0;

    while num_bytes < size {
        let c = char::from(file.read_u8().unwrap());
        if c == char::from(0) {
            break;
        }
        zstring.push(c);
        num_bytes += 1;
    }

    zstring
}

#[derive(Clone)]
pub struct LTEX {
    pub id: String,
    pub landscape_id: u32,
    pub filename: String,
}

impl LTEX {
    fn from_reader<T: Reader>(file: &mut T, end_of_record: u64, record_name: &str) -> Self {
        let mut id = None;
        let mut landscape_id = None;
        let mut filename = None;

        loop {
            let field = FieldHeader::from_reader(file);
            let size = field.size();

            if DEBUG {
                println!(
                    "{} IDX={} SIZE={}",
                    field.name(),
                    file.stream_position().unwrap(),
                    size
                );
            }

            match &field.type_tag {
                b"INTV" => {
                    landscape_id = Some(file.read_u32::<LittleEndian>().unwrap());
                }
                b"NAME" => {
                    id = Some(from_zstring(file, size));
                }
                b"DATA" => {
                    filename = Some(from_zstring(file, size));
                }
                _ => {
                    field.skip(file);
                    if DEBUG {
                        eprintln!(
                            "Found unknown field {} in record {}",
                            field.name(),
                            record_name
                        )
                    }
                }
            }

            if file.stream_position().unwrap() == end_of_record {
                // No more fields.
                break;
            }
        }

        Self {
            id: id.unwrap(),
            landscape_id: landscape_id.unwrap(),
            filename: filename.unwrap(),
        }
    }
}

impl LAND {
    fn from_reader<T: Reader>(file: &mut T, end_of_record: u64, record_name: &str) -> Self {
        let mut data = Self {
            coordinates: default(),
            included_data: DataFlags::NONE,
            vertex_normals: default(),
            height_data: default(),
            world_map_height: default(),
            vertex_colors: default(),
            vertex_textures: default(),
        };

        loop {
            let field = FieldHeader::from_reader(file);
            let size = field.size();

            if DEBUG {
                println!(
                    "{} IDX={} SIZE={}",
                    field.name(),
                    file.stream_position().unwrap(),
                    size
                );
            }

            match &field.type_tag {
                b"INTV" => {
                    assert_eq!(size, 8);
                    data.coordinates = Vec2::from_reader(file);
                }
                b"DATA" => {
                    assert_eq!(size, 4);
                    data.included_data = DataFlags::from_reader(file);
                }
                b"VNML" => {
                    assert_eq!(size, 12675);
                    if data.included_data.contains(DataFlags::VNML_VHGT_WNAM) {
                        data.vertex_normals = Some(from_reader::<_, _, 12675>(file));
                    } else {
                        field.skip(file);
                    }
                }
                b"VHGT" => {
                    assert_eq!(size, 4232);
                    if data.included_data.contains(DataFlags::VNML_VHGT_WNAM) {
                        data.height_data = Some(HeightData::from_reader(file));
                    } else {
                        field.skip(file);
                    }
                }
                b"WNAM" => {
                    assert_eq!(size, 81);
                    if data.included_data.contains(DataFlags::VNML_VHGT_WNAM) {
                        data.world_map_height = Some(from_reader::<_, _, 81>(file));
                    } else {
                        field.skip(file);
                    }
                }
                b"VCLR" => {
                    assert_eq!(size, 12675);
                    if data.included_data.contains(DataFlags::VCLR) {
                        data.vertex_colors = Some(from_reader::<_, _, 12675>(file));
                    } else {
                        field.skip(file);
                    }
                }
                b"VTEX" => {
                    assert_eq!(size, 512);
                    if data.included_data.contains(DataFlags::VTEX) {
                        data.vertex_textures = Some(from_reader::<_, _, 512>(file));
                    } else {
                        field.skip(file);
                    }
                }
                _ => {
                    field.skip(file);
                    if DEBUG {
                        eprintln!(
                            "Found unknown field {} in record {}",
                            field.name(),
                            record_name
                        )
                    }
                }
            }

            if file.stream_position().unwrap() == end_of_record {
                // No more fields.
                break;
            }
        }

        data
    }
}

pub struct ESData {
    pub plugin: String,
    pub num_records: HashMap<u32, usize>,
    pub records: AnyMap,
}

impl ESData {
    pub fn new(plugin: String) -> Self {
        Self {
            plugin,
            num_records: HashMap::new(),
            records: AnyMap::new(),
        }
    }
}

fn add_record_to<T: 'static>(data: &mut ESData, id: u32, record: T) {
    if !data.records.contains::<Vec<T>>() {
        let num_records = *data.num_records.get(&id).unwrap();
        data.records.insert(Vec::<T>::with_capacity(num_records));
    }

    let of_type = data.records.get_mut::<Vec<T>>().unwrap();
    of_type.push(record);
}

fn parse_next_record<T: Reader>(file: &mut T, data: &mut ESData) -> u64 {
    let record = RecordHeader::from_reader(file);
    let file_pos = file.stream_position().unwrap();
    let end_of_record = file_pos + record.data_size as u64;

    if DEBUG {
        println!("{} IDX={} SIZE={}", record.name(), file_pos, record.size());
    }

    if &record.type_tag == b"LAND" {
        add_record_to(
            data,
            record.id(),
            LAND::from_reader(file, end_of_record, record.name()),
        );
    } else if &record.type_tag == b"LTEX" {
        add_record_to(
            data,
            record.id(),
            LTEX::from_reader(file, end_of_record, record.name()),
        );
    } else {
        // Skip unknown record.
        record.skip(file);
    }

    file.stream_position().unwrap()
}

fn count_record_types<T: Reader>(file: &mut T, records: &mut ESData) {
    let file_len = file.seek(SeekFrom::End(0)).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();

    loop {
        let record = RecordHeader::from_reader(file);
        *records.num_records.entry(record.id()).or_insert(0) += 1;
        record.skip(file);
        if file.stream_position().unwrap() == file_len {
            break;
        }
    }
}

pub fn parse_records(plugin: &str) -> Box<ESData> {
    let data_file_path = format!("Data Files/{}", plugin);
    let path = Path::new(&data_file_path);

    println!("Parsing records for {:?}", path);

    let mut file = BufReader::new(File::open(path).unwrap());

    let mut data = Box::new(ESData::new(plugin.to_string()));
    count_record_types(&mut file, &mut data);

    let file_len = file.seek(SeekFrom::End(0)).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();

    loop {
        let file_pos = parse_next_record(&mut file, &mut data);
        if file_pos == file_len {
            // No more records.
            break;
        }
    }

    data
}
