//! Liest Blöcke aus Minecraft-Welten (Anvil-Format, Region-Dateien `.mca`).
//! Unterstützt 1.13 bis heute; ältere Welten (numerische Block-IDs) nicht.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::nbt::{self, Tag};
use crate::error::{Error, Result};

/// DataVersion von 1.13 (erste Version mit Block-Paletten).
const DV_1_13: i64 = 1451;
/// Ab 1.16 (20w17a) überspannen Block-Indizes keine Long-Grenzen mehr.
const DV_NO_SPAN: i64 = 2529;
/// Ab 1.18 (21w43a) neues Chunk-Format ohne „Level“.
const DV_1_18: i64 = 2844;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Overworld,
    Nether,
    End,
}

impl Dimension {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().trim_start_matches("minecraft:") {
            "" | "overworld" | "oberwelt" | "0" => Ok(Self::Overworld),
            "nether" | "the_nether" | "-1" => Ok(Self::Nether),
            "end" | "the_end" | "1" => Ok(Self::End),
            other => Err(Error::msg(format!("Unbekannte Dimension „{other}“ (overworld, nether, end)"))),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Overworld => "Oberwelt",
            Self::Nether => "Nether",
            Self::End => "End",
        }
    }

    /// Region-Ordner; berücksichtigt auch Bukkit/Spigot/Paper (`welt_nether/DIM-1`).
    pub fn region_dir(self, world: &Path) -> PathBuf {
        let (sub, suffix) = match self {
            Self::Overworld => return world.join("region"),
            Self::Nether => ("DIM-1", "_nether"),
            Self::End => ("DIM1", "_the_end"),
        };
        let vanilla = world.join(sub).join("region");
        if vanilla.is_dir() {
            return vanilla;
        }
        let name = world.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let bukkit = world.with_file_name(format!("{name}{suffix}")).join(sub).join("region");
        if bukkit.is_dir() {
            bukkit
        } else {
            vanilla
        }
    }
}

enum Blocks {
    Uniform(u16),
    Full(Box<[u16]>),
}

struct Section {
    blocks: Blocks,
    biomes: Vec<u16>,
}

impl Section {
    fn get(&self, x: usize, y: usize, z: usize) -> u16 {
        match &self.blocks {
            Blocks::Uniform(id) => *id,
            Blocks::Full(ids) => ids[(y << 8) | (z << 4) | x],
        }
    }
}

struct Chunk {
    /// Index = Sektions-Y; Wert `None` = leere Sektion.
    sections: HashMap<i32, Section>,
}

/// Liest Blöcke einer Dimension mit Zwischenspeicher für Regionen und Chunks.
pub struct WorldReader {
    region_dir: PathBuf,
    regions: HashMap<(i32, i32), Option<Vec<u8>>>,
    chunks: HashMap<(i32, i32), Option<Chunk>>,
    names: Vec<String>,
    ids: HashMap<String, u16>,
    pub min_y: i32,
    pub max_y: i32,
    air: [u16; 3],
}

pub const UNKNOWN: &str = "?";

impl WorldReader {
    pub fn open(world: &Path, dim: Dimension) -> Result<Self> {
        let region_dir = dim.region_dir(world);
        if !region_dir.is_dir() {
            return Err(Error::msg(format!(
                "Keine Daten für {} gefunden ({}).",
                dim.label(),
                region_dir.display()
            )));
        }
        let mut r = Self {
            region_dir,
            regions: HashMap::new(),
            chunks: HashMap::new(),
            names: Vec::new(),
            ids: HashMap::new(),
            min_y: i32::MAX,
            max_y: i32::MIN,
            air: [0; 3],
        };
        r.air = [r.intern("air"), r.intern("cave_air"), r.intern("void_air")];
        Ok(r)
    }

    fn intern(&mut self, name: &str) -> u16 {
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        let id = self.names.len() as u16;
        self.names.push(name.to_string());
        self.ids.insert(name.to_string(), id);
        id
    }

    pub fn name(&self, id: u16) -> &str {
        self.names.get(id as usize).map(String::as_str).unwrap_or(UNKNOWN)
    }

    pub fn is_air(&self, id: u16) -> bool {
        self.air.contains(&id)
    }

    fn region(&mut self, rx: i32, rz: i32) -> Option<&Vec<u8>> {
        let dir = &self.region_dir;
        self.regions
            .entry((rx, rz))
            .or_insert_with(|| std::fs::read(dir.join(format!("r.{rx}.{rz}.mca"))).ok())
            .as_ref()
    }

    fn read_chunk_nbt(&mut self, cx: i32, cz: i32) -> Option<Tag> {
        let data = self.region(cx.div_euclid(32), cz.div_euclid(32))?;
        let idx = 4 * ((cx.rem_euclid(32) + cz.rem_euclid(32) * 32) as usize);
        let header = data.get(idx..idx + 4)?;
        let offset = ((header[0] as usize) << 16 | (header[1] as usize) << 8 | header[2] as usize) * 4096;
        if offset == 0 {
            return None;
        }
        let len = u32::from_be_bytes(data.get(offset..offset + 4)?.try_into().ok()?) as usize;
        let compression = *data.get(offset + 4)?;
        let payload = data.get(offset + 5..offset + 4 + len)?;
        use std::io::Read;
        let mut raw = Vec::new();
        match compression {
            1 => flate2::read::GzDecoder::new(payload).read_to_end(&mut raw).ok()?,
            2 => flate2::read::ZlibDecoder::new(payload).read_to_end(&mut raw).ok()?,
            3 => {
                raw = payload.to_vec();
                raw.len()
            }
            _ => return None, // externe (.mcc) oder LZ4-Chunks: selten, überspringen
        };
        nbt::parse(&raw).ok()
    }

    fn load_chunk(&mut self, cx: i32, cz: i32) -> Option<Chunk> {
        let root = self.read_chunk_nbt(cx, cz)?;
        self.parse_chunk(&root)
    }

    fn parse_chunk(&mut self, root: &Tag) -> Option<Chunk> {
        let dv = root.get("DataVersion").and_then(Tag::as_i64).unwrap_or(0);
        if dv != 0 && dv < DV_1_13 {
            return None;
        }
        let modern = dv >= DV_1_18 || root.get("sections").is_some();
        let level = if modern { root } else { root.get("Level")? };

        // Nur fertig generierte Chunks verwenden.
        if let Some(status) = level.get("Status").and_then(Tag::as_str) {
            let s = status.trim_start_matches("minecraft:");
            if modern && s != "full" {
                return None;
            }
        }

        let list = level.get(if modern { "sections" } else { "Sections" })?.as_list()?;
        let mut sections = HashMap::new();
        for sec in list {
            let Some(y) = sec.get("Y").and_then(Tag::as_i64) else { continue };
            let y = y as i32;
            let (palette, data) = if modern {
                let bs = sec.get("block_states");
                (bs.and_then(|b| b.get("palette")), bs.and_then(|b| b.get("data")))
            } else {
                (sec.get("Palette"), sec.get("BlockStates"))
            };
            let Some(palette) = palette.and_then(Tag::as_list) else { continue };
            let ids: Vec<u16> = palette
                .iter()
                .map(|p| {
                    let name = p.get("Name").and_then(Tag::as_str).unwrap_or(UNKNOWN).to_string();
                    self.intern(&name)
                })
                .collect();
            let blocks = match (ids.len(), data.and_then(Tag::as_long_array)) {
                (0, _) => continue,
                (1, _) | (_, None) => Blocks::Uniform(ids[0]),
                (n, Some(longs)) => {
                    let bits = (usize::BITS - (n - 1).leading_zeros()).max(4) as usize;
                    Blocks::Full(unpack(longs, bits, 4096, dv >= DV_NO_SPAN || modern, &ids))
                }
            };
            let biomes = if modern {
                sec.path(&["biomes", "palette"])
                    .and_then(Tag::as_list)
                    .map(|l| l.iter().filter_map(Tag::as_str).map(|b| self.intern(b)).collect())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            self.min_y = self.min_y.min(y * 16);
            self.max_y = self.max_y.max(y * 16 + 15);
            sections.insert(y, Section { blocks, biomes });
        }
        Some(Chunk { sections })
    }

    fn chunk(&mut self, cx: i32, cz: i32) -> Option<&Chunk> {
        if !self.chunks.contains_key(&(cx, cz)) {
            let c = self.load_chunk(cx, cz);
            self.chunks.insert((cx, cz), c);
        }
        self.chunks.get(&(cx, cz)).and_then(Option::as_ref)
    }

    /// Ist der Chunk an dieser Blockposition generiert?
    pub fn has_column(&mut self, x: i32, z: i32) -> bool {
        self.chunk(x.div_euclid(16), z.div_euclid(16)).is_some()
    }

    /// Block-ID an einer Position; `None`, wenn der Chunk nicht generiert ist.
    pub fn block(&mut self, x: i32, y: i32, z: i32) -> Option<u16> {
        let air = self.air[0];
        let chunk = self.chunk(x.div_euclid(16), z.div_euclid(16))?;
        Some(match chunk.sections.get(&y.div_euclid(16)) {
            Some(s) => s.get(x.rem_euclid(16) as usize, y.rem_euclid(16) as usize, z.rem_euclid(16) as usize),
            None => air,
        })
    }

    /// Oberster Block, der keine Luft ist: (y, Block-ID).
    pub fn surface(&mut self, x: i32, z: i32) -> Option<(i32, u16)> {
        let air = self.air;
        let chunk = self.chunk(x.div_euclid(16), z.div_euclid(16))?;
        let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
        let mut ys: Vec<i32> = chunk.sections.keys().copied().collect();
        ys.sort_unstable_by(|a, b| b.cmp(a));
        for sy in ys {
            let s = &chunk.sections[&sy];
            if let Blocks::Uniform(id) = s.blocks {
                if air.contains(&id) {
                    continue;
                }
                return Some((sy * 16 + 15, id));
            }
            for ly in (0..16).rev() {
                let id = s.get(lx, ly, lz);
                if !air.contains(&id) {
                    return Some((sy * 16 + ly as i32, id));
                }
            }
        }
        None
    }

    /// Biome der Sektion an dieser Position (nur 1.18+).
    pub fn biomes_at(&mut self, x: i32, y: i32, z: i32) -> Vec<u16> {
        self.chunk(x.div_euclid(16), z.div_euclid(16))
            .and_then(|c| c.sections.get(&y.div_euclid(16)))
            .map(|s| s.biomes.clone())
            .unwrap_or_default()
    }
}

/// Entpackt Paletten-Indizes aus Long-Arrays.
fn unpack(longs: &[i64], bits: usize, count: usize, no_span: bool, ids: &[u16]) -> Box<[u16]> {
    let mask = (1u64 << bits) - 1;
    let fallback = ids[0];
    let lookup = |v: u64| ids.get(v as usize).copied().unwrap_or(fallback);
    let mut out = vec![fallback; count];
    if no_span {
        let per_long = 64 / bits;
        for (i, slot) in out.iter_mut().enumerate() {
            let Some(&l) = longs.get(i / per_long) else { break };
            *slot = lookup((l as u64 >> ((i % per_long) * bits)) & mask);
        }
    } else {
        for (i, slot) in out.iter_mut().enumerate() {
            let bit = i * bits;
            let (li, off) = (bit / 64, bit % 64);
            let Some(&l) = longs.get(li) else { break };
            let mut v = (l as u64) >> off;
            if off + bits > 64 {
                v |= (*longs.get(li + 1).unwrap_or(&0) as u64) << (64 - off);
            }
            *slot = lookup(v & mask);
        }
    }
    out.into_boxed_slice()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prüft echte Weltdateien, falls `ABLC_TEST_RESOURCES` auf einen Ordner
    /// mit `.mca`-/`.nbt`-Dateien zeigt (z. B. fastnbt/fastanvil/resources).
    #[test]
    fn real_world_files() {
        let Ok(dir) = std::env::var("ABLC_TEST_RESOURCES") else { return };
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let tmp = std::env::temp_dir().join(format!("ablc-world-{name}"));
            std::fs::create_dir_all(tmp.join("region")).unwrap();
            let mut r = WorldReader::open(&tmp, Dimension::Overworld).unwrap();
            let mut chunks = Vec::new();
            if name.ends_with(".mca") {
                std::fs::copy(&path, tmp.join("region/r.0.0.mca")).unwrap();
                for cz in 0..32 {
                    for cx in 0..32 {
                        if let Some(t) = r.read_chunk_nbt(cx, cz) {
                            chunks.push(t);
                        }
                    }
                }
            } else if let Ok(t) = nbt::parse_compressed(&std::fs::read(&path).unwrap()) {
                chunks.push(t);
            }
            let mut parsed = 0;
            let mut counts: HashMap<u16, usize> = HashMap::new();
            for t in &chunks {
                if let Some(c) = r.parse_chunk(t) {
                    parsed += 1;
                    for s in c.sections.values() {
                        for y in 0..16 {
                            for z in 0..16 {
                                for x in 0..16 {
                                    *counts.entry(s.get(x, y, z)).or_default() += 1;
                                }
                            }
                        }
                    }
                }
            }
            let mut top: Vec<_> = counts.into_iter().collect();
            top.sort_by_key(|e| std::cmp::Reverse(e.1));
            let names: Vec<String> = top.iter().take(8).map(|(id, n)| format!("{}:{n}", r.name(*id))).collect();
            let dv = chunks.first().and_then(|t| t.get("DataVersion")).and_then(Tag::as_i64);
            println!("{name}: DataVersion {dv:?}, {} Chunks gelesen, {parsed} ausgewertet, Höhe {}..{}, {}", chunks.len(), r.min_y, r.max_y, names.join(", "));
            assert!(top.iter().all(|(id, _)| !r.name(*id).is_empty()));
        }
    }

    #[test]
    fn unpack_no_span() {
        // 4 Bit pro Eintrag, 16 Einträge pro Long: 0,1,2,3,…
        let mut l: u64 = 0;
        for i in 0..16u64 {
            l |= (i % 3) << (i * 4);
        }
        let ids = [10, 11, 12];
        let out = unpack(&[l as i64], 4, 16, true, &ids);
        assert_eq!(&out[..6], &[10, 11, 12, 10, 11, 12]);
    }

    #[test]
    fn unpack_spanning() {
        // 5 Bit pro Eintrag; Eintrag 12 überspannt Long 0 und 1 (Bits 60–64).
        let bits = 5;
        let values: Vec<u64> = (0..26).map(|i| i % 17).collect();
        let mut longs = vec![0u64; 3];
        for (i, v) in values.iter().enumerate() {
            let bit = i * bits;
            longs[bit / 64] |= v << (bit % 64);
            if bit % 64 + bits > 64 {
                longs[bit / 64 + 1] |= v >> (64 - bit % 64);
            }
        }
        let ids: Vec<u16> = (0..17).collect();
        let longs: Vec<i64> = longs.into_iter().map(|l| l as i64).collect();
        let out = unpack(&longs, bits, 26, false, &ids);
        let expected: Vec<u16> = values.iter().map(|&v| v as u16).collect();
        assert_eq!(&out[..], &expected[..]);
    }
}
