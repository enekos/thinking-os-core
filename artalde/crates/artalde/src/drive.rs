//! `artalde drive <script>`: run the real window offscreen and script it —
//! pointer, keys, text, waits and screenshots — with every side effect
//! logged instead of done (`App::dry_run`). For UI review and regression
//! hunting without a visible window or a stolen focus.
//!
//! One step per line, a line starting with `#` is a comment; coordinates are in points:
//!
//! ```text
//! size 1440x900        window size (before anything else runs)
//! scale 2              pixels per point of the shots
//! wait 3               let the feed and gh workers deliver, in seconds
//! move 600,270         hover
//! click 600,270        press and release (also dclick, rclick)
//! scroll 600,400 -300  wheel at a point, in points
//! type hello           text into the focused field
//! key cmd+k            a key with optional cmd/shift/alt/ctrl
//! cmd pr.open o/r#1    run a palette command
//! shot out/main.png    save what is on screen
//! ```

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use artalde_core::command::Command;
use artalde_core::store::Store;
use eframe::egui::{self, Pos2, pos2, vec2};
use egui_kittest::Harness;

use crate::app::App;

/// Frame interval while waiting on workers.
const TICK: Duration = Duration::from_millis(30);

pub fn run(store: Store, config_dir: std::path::PathBuf, script: &Path) -> anyhow::Result<()> {
    let text =
        std::fs::read_to_string(script).with_context(|| format!("reading {}", script.display()))?;
    let steps: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let mut size = vec2(1440.0, 900.0);
    let mut scale = 2.0;
    for (_, line) in &steps {
        match line.split_once(' ') {
            Some(("size", v)) => size = parse_size(v)?,
            Some(("scale", v)) => scale = v.trim().parse()?,
            _ => {}
        }
    }
    let mut h = Harness::builder()
        .with_size(size)
        .with_pixels_per_point(scale)
        .with_max_steps(u64::MAX)
        .wgpu()
        .build_eframe(|cc| {
            let mut app = App::new(cc, store, config_dir).expect("artalde app starts offscreen");
            app.dry_run = true;
            app
        });
    h.step();
    for (n, line) in steps {
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        step(&mut h, verb, rest).with_context(|| format!("line {n}: {line}"))?;
    }
    Ok(())
}

fn step(h: &mut Harness<'_, App>, verb: &str, rest: &str) -> anyhow::Result<()> {
    match verb {
        "size" | "scale" => {}
        "wait" => {
            let until = Instant::now() + Duration::from_secs_f64(rest.parse()?);
            while Instant::now() < until {
                h.step();
                std::thread::sleep(TICK);
            }
        }
        "move" => {
            h.hover_at(parse_pos(rest)?);
            settle(h);
        }
        "click" | "dclick" | "rclick" => {
            let at = parse_pos(rest)?;
            let button = if verb == "rclick" {
                egui::PointerButton::Secondary
            } else {
                egui::PointerButton::Primary
            };
            h.hover_at(at);
            h.step();
            for _ in 0..if verb == "dclick" { 2 } else { 1 } {
                for pressed in [true, false] {
                    h.event(egui::Event::PointerButton {
                        pos: at,
                        button,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
                    h.step();
                }
            }
            settle(h);
        }
        "scroll" => {
            let (at, dy) = rest.rsplit_once(' ').context("scroll X,Y DY")?;
            h.hover_at(parse_pos(at)?);
            h.step();
            h.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, dy.parse()?),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: egui::Modifiers::NONE,
            });
            settle(h);
        }
        "type" => {
            h.event(egui::Event::Text(rest.to_owned()));
            settle(h);
        }
        "key" => {
            let (mods, key) = parse_key(rest)?;
            h.key_press_modifiers(mods, key);
            settle(h);
        }
        "cmd" => {
            let (name, arg) = rest.split_once(' ').unwrap_or((rest, ""));
            let arg = (!arg.is_empty()).then_some(arg);
            h.state_mut().queue.push(Command::new(name, arg));
            settle(h);
        }
        "shot" => {
            settle(h);
            let img = h.render().map_err(anyhow::Error::msg)?;
            let path = Path::new(rest);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let (w, hgt) = (img.width() as usize, img.height() as usize);
            crate::app::save_png(path, img.as_raw(), w, hgt)?;
            println!("{rest}");
        }
        other => bail!("unknown step {other:?}"),
    }
    Ok(())
}

/// A few frames so hover, popups and layout catch up with the input.
fn settle(h: &mut Harness<'_, App>) {
    for _ in 0..4 {
        h.step();
    }
}

fn parse_size(v: &str) -> anyhow::Result<egui::Vec2> {
    let (w, h) = v.trim().split_once('x').context("size WxH")?;
    Ok(vec2(w.parse()?, h.parse()?))
}

fn parse_pos(v: &str) -> anyhow::Result<Pos2> {
    let (x, y) = v.trim().split_once(',').context("X,Y")?;
    Ok(pos2(x.trim().parse()?, y.trim().parse()?))
}

fn parse_key(v: &str) -> anyhow::Result<(egui::Modifiers, egui::Key)> {
    let mut mods = egui::Modifiers::NONE;
    let mut parts: Vec<&str> = v.split('+').collect();
    let name = parts.pop().context("key name")?;
    for m in parts {
        match m.to_ascii_lowercase().as_str() {
            "cmd" => {
                mods.mac_cmd = true;
                mods.command = true;
            }
            "shift" => mods.shift = true,
            "alt" | "opt" => mods.alt = true,
            "ctrl" => mods.ctrl = true,
            other => bail!("unknown modifier {other:?}"),
        }
    }
    let key = egui::Key::from_name(name)
        .or_else(|| egui::Key::from_name(&name.to_ascii_uppercase()))
        .with_context(|| format!("unknown key {name:?}"))?;
    Ok((mods, key))
}
