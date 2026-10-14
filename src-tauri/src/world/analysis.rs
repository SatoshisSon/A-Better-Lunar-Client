//! Wandelt Weltdaten in kompakten Text, den Claude gut lesen kann.

use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;

use super::anvil::{Dimension, WorldReader};
use super::{players, read_level, WorldEntry};
use crate::error::{Error, Result};

/// Größte Seitenlänge für Gebietsübersichten (in Blöcken).
pub const MAX_AREA: i32 = 512;
/// Größtes Volumen für Block-Ausschnitte.
pub const MAX_VOLUME: i64 = 48 * 48 * 48;
/// Größte Kartenbreite in Zeichen.
const MAP_CELLS: i32 = 64;

const SYMBOLS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789#$%&*+=@";

/// Typische Blöcke von Spielerbauten – Hinweis auf vorhandene Gebäude.
const BUILD_HINTS: &[&str] = &[
    "planks", "glass", "brick", "concrete", "stairs", "slab", "wall", "fence", "door", "torch", "lantern",
    "carpet", "wool", "chest", "crafting_table", "furnace", "bed", "bookshelf", "polished", "smooth_",
    "cut_", "chiseled", "lamp", "rail", "barrel", "trapdoor", "_bars", "quartz", "terracotta", "banner",
];

/// Pflanzen und Bäume, die beim Ermitteln der Bodenhöhe übersprungen werden.
fn is_vegetation(name: &str) -> bool {
    const PARTS: &[&str] = &[
        "leaves", "_log", "_wood", "vine", "grass", "fern", "flower", "tulip", "dandelion", "poppy", "orchid",
        "allium", "bluet", "daisy", "lily", "sunflower", "lilac", "rose_bush", "peony", "dead_bush", "berry_bush",
        "mushroom", "bamboo", "sugar_cane", "snow", "azalea", "roots", "propagule", "moss_carpet", "petals",
        "sapling", "cocoa", "bee_nest",
    ];
    PARTS.iter().any(|p| name.contains(p)) && name != "grass_block" && !name.contains("snow_block")
        && name != "mushroom_stem" && !name.contains("_planks")
}

fn is_build_block(name: &str) -> bool {
    BUILD_HINTS.iter().any(|h| name.contains(h)) && !name.contains("infested")
}

pub fn world_info(w: &WorldEntry) -> Result<String> {
    let path = Path::new(&w.path);
    let level = read_level(path);
    let mut s = String::new();
    writeln!(s, "Welt: {} ({})", w.name, if w.source == "server" { "Server" } else { "Einzelspieler" }).ok();
    if let Some(p) = &w.profile {
        writeln!(s, "Profil: {p}").ok();
    }
    if let Some(l) = &level {
        if let Some(v) = &l.version {
            writeln!(s, "Minecraft-Version: {v}").ok();
        }
        if let Some(g) = &l.game_mode {
            writeln!(s, "Spielmodus: {g}").ok();
        }
        if let Some([x, y, z]) = l.spawn {
            writeln!(s, "Spawn: x={x} y={y} z={z}").ok();
        }
        if let Some(t) = l.day_time {
            let tick = t.rem_euclid(24000);
            let hour = (tick / 1000 + 6) % 24;
            writeln!(s, "Tag {}, ca. {hour:02}:00 Uhr{}", t / 24000 + 1, if l.raining { ", Regen" } else { "" }).ok();
        }
        if l.data_version.is_some_and(|dv| dv < 1451) {
            writeln!(s, "Hinweis: Welten vor 1.13 können nicht gelesen werden.").ok();
        }
    }

    for dim in [Dimension::Overworld, Dimension::Nether, Dimension::End] {
        let dir = dim.region_dir(path);
        let Ok(files) = std::fs::read_dir(&dir) else { continue };
        let mut bounds: Option<(i32, i32, i32, i32)> = None;
        let mut count = 0;
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            let parts: Vec<&str> = name.split('.').collect();
            if parts.len() != 4 || parts[0] != "r" || parts[3] != "mca" {
                continue;
            }
            let (Ok(rx), Ok(rz)) = (parts[1].parse::<i32>(), parts[2].parse::<i32>()) else { continue };
            if f.metadata().map(|m| m.len() <= 8192).unwrap_or(true) {
                continue;
            }
            count += 1;
            bounds = Some(match bounds {
                None => (rx, rz, rx, rz),
                Some((a, b, c, d)) => (a.min(rx), b.min(rz), c.max(rx), d.max(rz)),
            });
        }
        if let Some((x1, z1, x2, z2)) = bounds {
            writeln!(
                s,
                "{}: {count} Regionen, erkundet etwa x {}..{}, z {}..{}",
                dim.label(),
                x1 * 512,
                x2 * 512 + 511,
                z1 * 512,
                z2 * 512 + 511
            )
            .ok();
        }
    }

    let ps = players(path, level.as_ref());
    if !ps.is_empty() {
        writeln!(s, "Spieler (letzte gespeicherte Position):").ok();
        for p in ps.iter().take(30) {
            writeln!(s, "  {}: x={:.0} y={:.0} z={:.0} ({})", p.name, p.pos[0], p.pos[1], p.pos[2], p.dimension).ok();
        }
    }
    writeln!(s, "\nHinweis: Daten stammen aus dem zuletzt gespeicherten Stand der Welt.").ok();
    Ok(s)
}

fn symbol_table(ranked: &[u16]) -> HashMap<u16, char> {
    ranked.iter().zip(SYMBOLS.chars()).map(|(&id, c)| (id, c)).collect()
}

fn normalize(x1: i32, x2: i32) -> (i32, i32) {
    (x1.min(x2), x1.max(x2))
}

/// Übersicht eines Gebiets: Höhen, Oberflächen, Biome, Karten.
pub fn area_summary(world: &Path, dim: Dimension, x1: i32, z1: i32, x2: i32, z2: i32) -> Result<String> {
    let (x1, x2) = normalize(x1, x2);
    let (z1, z2) = normalize(z1, z2);
    let (w, d) = (x2 - x1 + 1, z2 - z1 + 1);
    if w > MAX_AREA || d > MAX_AREA {
        return Err(Error::msg(format!("Gebiet zu groß (max. {MAX_AREA}×{MAX_AREA} Blöcke).")));
    }
    let mut r = WorldReader::open(world, dim)?;
    let water = ["water", "bubble_column", "seagrass", "tall_seagrass", "kelp", "kelp_plant"];

    // Oberfläche (was man von oben sieht) und Bodenhöhe (ohne Bäume und Pflanzen).
    let mut heights: Vec<Option<(i32, u16)>> = Vec::with_capacity((w * d) as usize);
    let mut ground: Vec<Option<i32>> = Vec::with_capacity((w * d) as usize);
    for z in z1..=z2 {
        for x in x1..=x2 {
            let top = r.surface(x, z);
            let g = top.map(|(mut y, mut id)| {
                while (is_vegetation(r.name(id)) || r.is_air(id)) && y > r.min_y {
                    y -= 1;
                    id = r.block(x, y, z).unwrap_or(id);
                }
                y
            });
            heights.push(top);
            ground.push(g);
        }
    }
    let known: Vec<(i32, u16)> = heights.iter().flatten().copied().collect();
    let mut s = String::new();
    writeln!(s, "Gebiet x {x1}..{x2}, z {z1}..{z2} ({w}×{d} Blöcke, {})", dim.label()).ok();
    if known.is_empty() {
        writeln!(s, "Dieses Gebiet wurde noch nicht generiert/erkundet.").ok();
        return Ok(s);
    }
    let generated = known.len() as f64 / heights.len() as f64;
    if generated < 0.999 {
        writeln!(s, "Generiert: {:.0}% (Rest noch nicht erkundet)", generated * 100.0).ok();
    }

    // Höhen (Boden, ohne Bäume)
    let ys: Vec<i32> = ground.iter().flatten().copied().collect();
    let (min, max) = (*ys.iter().min().unwrap(), *ys.iter().max().unwrap());
    let avg = ys.iter().map(|&y| y as f64).sum::<f64>() / ys.len() as f64;
    let sd = (ys.iter().map(|&y| (y as f64 - avg).powi(2)).sum::<f64>() / ys.len() as f64).sqrt();
    let flat = match sd {
        v if v < 1.5 => "sehr flach",
        v if v < 4.0 => "leicht hügelig",
        v if v < 10.0 => "hügelig",
        _ => "bergig/steil",
    };
    writeln!(s, "Bodenhöhe (ohne Bäume): min {min}, max {max}, Ø {avg:.1} ({flat}, Streuung {sd:.1})").ok();

    // Oberflächenblöcke
    let mut counts: HashMap<u16, usize> = HashMap::new();
    for &(_, id) in &known {
        *counts.entry(id).or_default() += 1;
    }
    let mut ranked: Vec<(u16, usize)> = counts.into_iter().collect();
    ranked.sort_by_key(|e| std::cmp::Reverse(e.1));
    let total = known.len() as f64;
    let water_share: usize = ranked.iter().filter(|(id, _)| water.contains(&r.name(*id))).map(|x| x.1).sum();
    if water_share > 0 {
        writeln!(s, "Wasserfläche: {:.0}%", water_share as f64 / total * 100.0).ok();
    }
    let top: Vec<String> = ranked
        .iter()
        .take(15)
        .map(|(id, n)| format!("{} {:.0}%", r.name(*id), *n as f64 / total * 100.0))
        .collect();
    writeln!(s, "Häufigste Oberflächenblöcke: {}", top.join(", ")).ok();

    // Biome (1.18+): Stichprobe auf Oberflächenhöhe
    let mut biomes: HashMap<u16, usize> = HashMap::new();
    let step = ((w.max(d)) / 16).max(4);
    for z in (z1..=z2).step_by(step as usize) {
        for x in (x1..=x2).step_by(step as usize) {
            if let Some(&Some((y, _))) = heights.get(((z - z1) * w + (x - x1)) as usize) {
                for b in r.biomes_at(x, y, z) {
                    *biomes.entry(b).or_default() += 1;
                }
            }
        }
    }
    if !biomes.is_empty() {
        let mut b: Vec<_> = biomes.into_iter().collect();
        b.sort_by_key(|e| std::cmp::Reverse(e.1));
        writeln!(s, "Biome: {}", b.iter().take(8).map(|(id, _)| r.name(*id)).collect::<Vec<_>>().join(", ")).ok();
    }

    // Hinweise auf Bauten
    let mut builds: HashMap<u16, (usize, i64, i64)> = HashMap::new();
    for (i, h) in heights.iter().enumerate() {
        if let Some((_, id)) = h {
            if is_build_block(r.name(*id)) {
                let e = builds.entry(*id).or_default();
                e.0 += 1;
                e.1 += (x1 + i as i32 % w) as i64;
                e.2 += (z1 + i as i32 / w) as i64;
            }
        }
    }
    if !builds.is_empty() {
        let mut b: Vec<_> = builds.into_iter().collect();
        b.sort_by_key(|e| std::cmp::Reverse(e.1 .0));
        let list: Vec<String> = b
            .iter()
            .take(10)
            .map(|(id, (n, sx, sz))| format!("{} ×{n} (um x={}, z={})", r.name(*id), sx / *n as i64, sz / *n as i64))
            .collect();
        writeln!(s, "Bauwerke an der Oberfläche (von Spielern oder generiert, z. B. Dörfer/Ruinen): {}", list.join("; ")).ok();
    }

    // Karten (verkleinert)
    let cell = ((w.max(d) + MAP_CELLS - 1) / MAP_CELLS).max(1);
    let (cw, cd) = ((w + cell - 1) / cell, (d + cell - 1) / cell);
    let mut cells: Vec<Option<(f64, u16)>> = Vec::with_capacity((cw * cd) as usize);
    for cz in 0..cd {
        for cx in 0..cw {
            let mut cnt: HashMap<u16, usize> = HashMap::new();
            let (mut sum, mut n) = (0f64, 0usize);
            for z in cz * cell..((cz + 1) * cell).min(d) {
                for x in cx * cell..((cx + 1) * cell).min(w) {
                    let i = (z * w + x) as usize;
                    if let (Some((_, id)), Some(y)) = (heights[i], ground[i]) {
                        *cnt.entry(id).or_default() += 1;
                        sum += y as f64;
                        n += 1;
                    }
                }
            }
            cells.push(cnt.into_iter().max_by_key(|e| e.1).map(|(id, _)| (sum / n as f64, id)));
        }
    }
    let mut cell_ranked: HashMap<u16, usize> = HashMap::new();
    for (_, id) in cells.iter().flatten() {
        *cell_ranked.entry(*id).or_default() += 1;
    }
    let mut cr: Vec<_> = cell_ranked.into_iter().collect();
    cr.sort_by_key(|e| std::cmp::Reverse(e.1));
    let ranked_ids: Vec<u16> = cr.iter().map(|e| e.0).collect();
    let syms = symbol_table(&ranked_ids);

    // Nicht generierte Ränder abschneiden.
    let has = |cx: i32, cz: i32| cells[(cz * cw + cx) as usize].is_some();
    let cols: Vec<i32> = (0..cw).filter(|&cx| (0..cd).any(|cz| has(cx, cz))).collect();
    let rows: Vec<i32> = (0..cd).filter(|&cz| (0..cw).any(|cx| has(cx, cz))).collect();
    let (c0, c1) = (cols[0], *cols.last().unwrap());
    let (r0, r1) = (rows[0], *rows.last().unwrap());
    let grid: Vec<&[Option<(f64, u16)>]> = (r0..=r1)
        .map(|cz| &cells[(cz * cw + c0) as usize..=(cz * cw + c1) as usize])
        .collect();
    let (mx1, mz1) = (x1 + c0 * cell, z1 + r0 * cell);
    let (mx2, mz2) = ((x1 + (c1 + 1) * cell - 1).min(x2), (z1 + (r1 + 1) * cell - 1).min(z2));

    writeln!(
        s,
        "\nMaterialkarte x {mx1}..{mx2}, z {mz1}..{mz2} (1 Zeichen = {cell}×{cell} Blöcke, Norden oben, x nach rechts):"
    )
    .ok();
    let legend: Vec<String> = ranked_ids
        .iter()
        .filter_map(|id| syms.get(id).map(|c| format!("{c}={}", r.name(*id))))
        .collect();
    writeln!(s, "Legende: {}  (Leerzeichen = nicht generiert)", legend.join(" ")).ok();
    for row in &grid {
        let line: String = row.iter().map(|c| c.and_then(|(_, id)| syms.get(&id).copied()).unwrap_or(' ')).collect();
        writeln!(s, "{}", line.trim_end()).ok();
    }

    // Skala ohne Ausreißer (2.–98. Perzentil), damit die Karte aussagekräftig bleibt.
    let mut sorted = ys.clone();
    sorted.sort_unstable();
    let lo = sorted[sorted.len() * 2 / 100];
    let hi = sorted[(sorted.len() * 98 / 100).min(sorted.len() - 1)];
    let (min, range) = (lo, (hi - lo).max(1) as f64);
    writeln!(s, "\nHöhenkarte Boden (0 = y {lo} oder tiefer, 9 = y {hi} oder höher):").ok();
    for row in &grid {
        let line: String = row
            .iter()
            .map(|c| match c {
                Some((h, _)) => char::from(b'0' + (((h - min as f64) / range) * 9.0).round().clamp(0.0, 9.0) as u8),
                None => ' ',
            })
            .collect();
        writeln!(s, "{}", line.trim_end()).ok();
    }
    Ok(s)
}

/// Exakter Block-Ausschnitt, Schicht für Schicht von unten nach oben.
#[allow(clippy::too_many_arguments)]
pub fn blocks(world: &Path, dim: Dimension, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32) -> Result<String> {
    let (x1, x2) = normalize(x1, x2);
    let (y1, y2) = normalize(y1, y2);
    let (z1, z2) = normalize(z1, z2);
    let volume = (x2 - x1 + 1) as i64 * (y2 - y1 + 1) as i64 * (z2 - z1 + 1) as i64;
    if volume > MAX_VOLUME {
        return Err(Error::msg(format!("Ausschnitt zu groß ({volume} Blöcke, max. {MAX_VOLUME}). Bitte verkleinern.")));
    }
    let mut r = WorldReader::open(world, dim)?;
    let (w, d) = ((x2 - x1 + 1) as usize, (z2 - z1 + 1) as usize);
    let mut layers: Vec<(i32, Vec<Option<u16>>)> = Vec::new();
    let mut counts: HashMap<u16, usize> = HashMap::new();
    for y in y1..=y2 {
        let mut layer = Vec::with_capacity(w * d);
        for z in z1..=z2 {
            for x in x1..=x2 {
                let b = r.block(x, y, z);
                if let Some(id) = b {
                    if !r.is_air(id) {
                        *counts.entry(id).or_default() += 1;
                    }
                }
                layer.push(b);
            }
        }
        layers.push((y, layer));
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by_key(|e| std::cmp::Reverse(e.1));
    if ranked.len() > SYMBOLS.chars().count() {
        return Err(Error::msg("Zu viele verschiedene Blöcke im Ausschnitt. Bitte verkleinern."));
    }
    let ids: Vec<u16> = ranked.iter().map(|e| e.0).collect();
    let syms = symbol_table(&ids);

    let mut s = String::new();
    writeln!(s, "Ausschnitt x {x1}..{x2}, y {y1}..{y2}, z {z1}..{z2} ({})", dim.label()).ok();
    writeln!(s, "Jede Schicht: Zeilen = z (Norden oben, von z={z1}), Spalten = x (von x={x1} nach Osten).").ok();
    let legend: Vec<String> = ranked.iter().map(|(id, n)| format!("{}={} ({n})", syms[id], r.name(*id))).collect();
    writeln!(s, "Legende: .=Luft ?=nicht generiert {}", legend.join(" ")).ok();

    let mut empty_run: Option<(i32, i32)> = None;
    let flush = |s: &mut String, run: &mut Option<(i32, i32)>| {
        if let Some((a, b)) = run.take() {
            if a == b {
                writeln!(s, "y={a}: nur Luft").ok();
            } else {
                writeln!(s, "y={a}..{b}: nur Luft").ok();
            }
        }
    };
    for (y, layer) in &layers {
        let all_air = layer.iter().all(|b| b.is_some_and(|id| r.is_air(id)));
        if all_air {
            empty_run = Some(match empty_run {
                Some((a, _)) => (a, *y),
                None => (*y, *y),
            });
            continue;
        }
        flush(&mut s, &mut empty_run);
        writeln!(s, "y={y}:").ok();
        for row in layer.chunks(w) {
            let line: String = row
                .iter()
                .map(|b| match b {
                    None => '?',
                    Some(id) if r.is_air(*id) => '.',
                    Some(id) => syms[id],
                })
                .collect();
            writeln!(s, "{line}").ok();
        }
    }
    flush(&mut s, &mut empty_run);
    Ok(s)
}

/// Sucht Blöcke (Teil des Namens, z. B. „chest“ oder „diamond_ore“).
pub fn find_blocks(world: &Path, dim: Dimension, query: &str, cx: i32, cz: i32, radius: i32) -> Result<String> {
    let radius = radius.clamp(1, 128);
    let q = query.trim().trim_start_matches("minecraft:").to_lowercase();
    if q.is_empty() {
        return Err(Error::msg("Bitte einen Blocknamen angeben."));
    }
    let mut r = WorldReader::open(world, dim)?;
    // Höhenbereich über die Chunks im Gebiet ermitteln.
    for z in ((cz - radius)..=(cz + radius)).step_by(16) {
        for x in ((cx - radius)..=(cx + radius)).step_by(16) {
            r.has_column(x, z);
        }
    }
    if r.min_y > r.max_y {
        return Ok("Dieses Gebiet wurde noch nicht generiert.".into());
    }
    let (min_y, max_y) = (r.min_y, r.max_y);
    let mut found: Vec<(String, i32, i32, i32)> = Vec::new();
    let mut total = 0usize;
    for y in min_y..=max_y {
        for z in (cz - radius)..=(cz + radius) {
            for x in (cx - radius)..=(cx + radius) {
                let Some(id) = r.block(x, y, z) else { continue };
                let name = r.name(id);
                if name.contains(&q) {
                    total += 1;
                    if found.len() < 200 {
                        found.push((name.to_string(), x, y, z));
                    }
                }
            }
        }
    }
    let mut s = String::new();
    writeln!(s, "Suche „{q}“ im Umkreis {radius} um x={cx}, z={cz} ({}): {total} Treffer", dim.label()).ok();
    found.sort_by_key(|f| (f.1 - cx).pow(2) + (f.3 - cz).pow(2));
    for (name, x, y, z) in &found {
        writeln!(s, "{name} bei x={x} y={y} z={z}").ok();
    }
    if total > found.len() {
        writeln!(s, "… und {} weitere (nächste zuerst gelistet)", total - found.len()).ok();
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::nbt::{self, Tag};

    /// Gibt die Analyse einer echten Region-Datei aus (`ABLC_TEST_REGION`).
    #[test]
    fn real_region_analysis() {
        let Ok(file) = std::env::var("ABLC_TEST_REGION") else { return };
        let data = std::fs::read(&file).unwrap();
        // Region-Koordinaten aus dem ersten Chunk bestimmen.
        let (mut rx, mut rz) = (0, 0);
        for i in 0..1024 {
            let h = &data[i * 4..i * 4 + 4];
            let off = ((h[0] as usize) << 16 | (h[1] as usize) << 8 | h[2] as usize) * 4096;
            if off == 0 {
                continue;
            }
            let len = u32::from_be_bytes(data[off..off + 4].try_into().unwrap()) as usize;
            let tag = nbt::parse_compressed(&data[off + 5..off + 4 + len]).unwrap();
            let x = tag.get("xPos").and_then(Tag::as_i64).unwrap() as i32;
            let z = tag.get("zPos").and_then(Tag::as_i64).unwrap() as i32;
            rx = x.div_euclid(32);
            rz = z.div_euclid(32);
            break;
        }
        let world = std::env::temp_dir().join("ablc-analysis-world");
        std::fs::create_dir_all(world.join("region")).unwrap();
        std::fs::copy(&file, world.join(format!("region/r.{rx}.{rz}.mca"))).unwrap();
        let (bx, bz) = (rx * 512, rz * 512);
        let t = std::time::Instant::now();
        println!("{}", area_summary(&world, Dimension::Overworld, bx, bz, bx + 511, bz + 511).unwrap());
        println!("[area_summary 512×512: {:?}]", t.elapsed());
        let t = std::time::Instant::now();
        let out = blocks(&world, Dimension::Overworld, bx + 100, 60, bz + 100, bx + 111, 72, bz + 107).unwrap();
        println!("{out}\n[blocks: {:?}]", t.elapsed());
        let t = std::time::Instant::now();
        println!("{}", find_blocks(&world, Dimension::Overworld, "diamond_ore", bx + 200, bz + 200, 64).unwrap().lines().take(5).collect::<Vec<_>>().join("\n"));
        println!("[find_blocks r=64: {:?}]", t.elapsed());
    }
}
