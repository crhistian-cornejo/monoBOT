//! The face a personal bot wears: a small soft character in three dimensions,
//! where a coding Mono keeps its pixel mascot. The bot's colour is its body,
//! its mascot is its species (a cat's ears, a robot's screen, a mushroom's
//! cap) and its role dresses it (the Principal's crown, the Inbox's cap, the
//! Analyst's glasses), so nothing new is stored.
//!
//! One body carries every species, and each role adds one prop, as momo's
//! characters do (github.com/mortspace/momo, MIT). Two rules keep them
//! tidy: whatever a species grows on top of its head (antennae, a crest, a
//! nose cone) gives way to a hat, and what a role carries is held in the
//! hand, never planted on the head.
//!
//! The character is a signed distance field marched in software, one ray per
//! sample, lit and written to plain RGBA. No shader is involved, so it is the
//! same picture on every platform. The desktop and the phone each render a
//! pose once, off the interface thread, and keep it.

use std::ops::{Add, Mul, Sub};

/// Changes with every edit to this file, so a picture kept between launches
/// (the phone keeps its renders on disk) is drawn again after the renderer
/// changes.
pub const REVISION: u64 = {
    let source = include_bytes!("lib.rs");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut at = 0;
    while at < source.len() {
        hash = (hash ^ source[at] as u64).wrapping_mul(0x0100_0000_01b3);
        at += 1;
    }
    hash
};

/// What marks a face as a persona where a mascot name is expected.
const PREFIX: &str = "3d:";

/// The face of a bot with this mascot and role, as `avatar` takes it.
pub fn key(mascot: &str, role: Option<&str>) -> String {
    format!("{PREFIX}{mascot}:{}", role.unwrap_or("custom"))
}

/// The persona a face names, or `None` for a pixel mascot's name.
pub fn parse(face: &str) -> Option<(Species, Outfit)> {
    let (mascot, role) = face.strip_prefix(PREFIX)?.split_once(':')?;
    Some((Species::of(mascot), Outfit::of(role)))
}

/// The pixel mascot inside a face: the name a persona was made from.
pub fn mascot(face: &str) -> &str {
    face.strip_prefix(PREFIX)
        .and_then(|rest| rest.split_once(':'))
        .map_or(face, |(mascot, _)| mascot)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Species {
    Invader,
    Ghost,
    Robot,
    Cat,
    Skull,
    Crab,
    Mushroom,
    Rocket,
    Dino,
    Frog,
}

impl Species {
    fn of(mascot: &str) -> Self {
        match mascot {
            "invader" => Self::Invader,
            "robot" => Self::Robot,
            "cat" => Self::Cat,
            "skull" => Self::Skull,
            "crab" => Self::Crab,
            "mushroom" => Self::Mushroom,
            "rocket" => Self::Rocket,
            "dino" => Self::Dino,
            "frog" => Self::Frog,
            _ => Self::Ghost,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outfit {
    Plain,
    Crown,
    Headset,
    PostCap,
    Flag,
    Quill,
    Glasses,
    TopHat,
    Mortarboard,
    Explorer,
    Bow,
    Beret,
    Phone,
    Captain,
    HardHat,
}

impl Outfit {
    fn of(role: &str) -> Self {
        match role {
            "principal" => Self::Crown,
            "assistant" => Self::Headset,
            "inbox" => Self::PostCap,
            "planner" => Self::Flag,
            "writer" => Self::Quill,
            "analyst" => Self::Glasses,
            "finance" => Self::TopHat,
            "documents" => Self::Mortarboard,
            "research" => Self::Explorer,
            "social" => Self::Bow,
            "images" => Self::Beret,
            "phone" => Self::Phone,
            "chief" => Self::Captain,
            "coder" => Self::HardHat,
            _ => Self::Plain,
        }
    }

    /// Sits on the crown of the head, so nothing else grows there.
    fn covers_top(self) -> bool {
        matches!(
            self,
            Self::Crown
                | Self::Headset
                | Self::PostCap
                | Self::TopHat
                | Self::Mortarboard
                | Self::Explorer
                | Self::Beret
                | Self::Captain
                | Self::HardHat
        )
    }

    /// How far a hat leans, in radians: none sits dead level.
    fn lean(self) -> f32 {
        match self {
            Self::Crown => 0.13,
            Self::PostCap => -0.07,
            Self::TopHat => -0.12,
            Self::Mortarboard => 0.08,
            Self::Explorer => 0.05,
            Self::Beret => -0.16,
            Self::Captain => 0.06,
            Self::HardHat => -0.05,
            _ => 0.,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Look {
    pub species: Species,
    pub outfit: Outfit,
    /// `0xRRGGBB`.
    pub color: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pose {
    Rest,
    /// Eyes shut.
    Blink,
    /// Mouth open, for the frames between `Rest` while it works or speaks.
    Talk,
}

/// The largest picture worth rendering; bigger avatars scale it.
pub const MAX_SIDE: u32 = 192;

/// The character as `side` × `side` straight-alpha RGBA, top row first.
/// Below 40 pixels it is a portrait (see `render_at`).
pub fn render(look: Look, pose: Pose, side: u32) -> Vec<u8> {
    render_at(look, pose, side, side < 40)
}

/// `render`, saying whether the picture is a portrait: head and shoulders,
/// with bolder eyes and no smile. Where a picture is shown small on a dense
/// screen it has pixels enough for the whole character and is still too
/// small to read it.
pub fn render_at(look: Look, pose: Pose, side: u32, portrait: bool) -> Vec<u8> {
    let side = side.clamp(1, MAX_SIDE);
    // Small pictures take more samples: their eyes are a few pixels wide.
    let grid: u32 = if side < 64 { 3 } else { 2 };
    let scene = Scene::new(look, pose, portrait);
    // A small picture is a portrait: head and shoulders fill it, so the
    // species and what it wears still read beside a name in a list.
    let (half, centre) = if portrait { PORTRAIT } else { (VIEW, 0.) };
    // Clear pixels keep the body's colour, so a picture drawn a little
    // larger or smaller blends its edge with that and not with black.
    let clear = scene.body.map(|c| (c.clamp(0., 1.) * 255.).round() as u8);
    let mut out = Vec::with_capacity((side * side * 4) as usize);
    for row in 0..side {
        for column in 0..side {
            let mut sum = [0f32; 4];
            for sample in 0..grid * grid {
                let (sx, sy) = (sample % grid, sample / grid);
                let at = |pixel: u32, sub: u32| {
                    (pixel as f32 + (sub as f32 + 0.5) / grid as f32) / side as f32
                };
                let x = (at(column, sx) * 2. - 1.) * half;
                let y = (1. - at(row, sy) * 2.) * half + centre;
                let [r, g, b, a] = scene.sample(x, y);
                sum[0] += r * a;
                sum[1] += g * a;
                sum[2] += b * a;
                sum[3] += a;
            }
            let alpha = sum[3] / (grid * grid) as f32;
            for (channel, clear) in sum[..3].iter().zip(clear) {
                out.push(if sum[3] > 0. {
                    ((channel / sum[3]).clamp(0., 1.) * 255.).round() as u8
                } else {
                    clear
                });
            }
            out.push((alpha.clamp(0., 1.) * 255.).round() as u8);
        }
    }
    out
}

/// Half the width of what the picture shows, in the character's units.
const VIEW: f32 = 1.06;
/// The same for a small picture, and how far above the middle it looks.
const PORTRAIT: (f32, f32) = (0.82, 0.14);
/// The character stands this far below the middle, leaving room for a hat.
const STAND: f32 = -0.04;

#[derive(Clone, Copy, Debug, PartialEq)]
struct V(f32, f32, f32);

impl Add for V {
    type Output = V;
    fn add(self, o: V) -> V {
        V(self.0 + o.0, self.1 + o.1, self.2 + o.2)
    }
}
impl Sub for V {
    type Output = V;
    fn sub(self, o: V) -> V {
        V(self.0 - o.0, self.1 - o.1, self.2 - o.2)
    }
}
impl Mul<f32> for V {
    type Output = V;
    fn mul(self, k: f32) -> V {
        V(self.0 * k, self.1 * k, self.2 * k)
    }
}
impl V {
    fn dot(self, o: V) -> f32 {
        self.0 * o.0 + self.1 * o.1 + self.2 * o.2
    }
    fn len(self) -> f32 {
        self.dot(self).sqrt()
    }
    fn unit(self) -> V {
        self * (1. / self.len().max(1e-6))
    }
    /// Mirrored onto the right side, for what the character has two of.
    fn mirrored(self) -> V {
        V(self.0.abs(), self.1, self.2)
    }
    /// Turned by `angle` about the line through `pivot` that points at the
    /// viewer, so a hat can lean.
    fn leaned(self, pivot: V, angle: f32) -> V {
        let (sin, cos) = angle.sin_cos();
        let q = self - pivot;
        V(q.0 * cos - q.1 * sin, q.0 * sin + q.1 * cos, q.2) + pivot
    }
}

type Rgb = [f32; 3];

/// `color` as the pixel mascots soften theirs: less saturated and lighter.
fn pastel(color: Rgb) -> Rgb {
    let [r, g, b] = color;
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let lightness = (max + min) / 2.;
    let spread = max - min;
    let (hue, saturation) = if spread < 1e-5 {
        (0., 0.)
    } else {
        let hue = if max == r {
            ((g - b) / spread).rem_euclid(6.)
        } else if max == g {
            (b - r) / spread + 2.
        } else {
            (r - g) / spread + 4.
        };
        (
            hue / 6.,
            spread / (1. - (2. * lightness - 1.).abs()).max(1e-5),
        )
    };
    let saturation = saturation * 0.70;
    let lightness = lightness + (0.80 - lightness) * 0.50;
    // Back to red, green and blue.
    let chroma = (1. - (2. * lightness - 1.).abs()) * saturation;
    let channel = |shift: f32| {
        let k = (shift + hue * 12.).rem_euclid(12.);
        lightness - chroma / 2. * (k - 3.).min(9. - k).clamp(-1., 1.)
    };
    [channel(0.), channel(8.), channel(4.)]
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0., 1.);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

const WHITE: Rgb = [1., 1., 1.];
const INK: Rgb = [0.10, 0.11, 0.15];
const GOLD: Rgb = [0.99, 0.78, 0.27];
const STEEL: Rgb = [0.62, 0.66, 0.72];
const CHARCOAL: Rgb = [0.21, 0.23, 0.28];
const RED: Rgb = [0.94, 0.38, 0.35];
const PINK: Rgb = [0.98, 0.56, 0.66];

/// What a surface is made of.
#[derive(Clone, Copy)]
struct Surface {
    color: Rgb,
    /// How sharp its highlight is, 0 (cloth) to 1 (polished).
    gloss: f32,
    /// The body itself, where the face and the tummy are drawn.
    skin: bool,
}

fn matter(color: Rgb, gloss: f32) -> Surface {
    Surface {
        color,
        gloss,
        skin: false,
    }
}

/// Keeps whichever surface is nearer.
fn closer(best: &mut (f32, Surface), distance: f32, surface: Surface) {
    if distance < best.0 {
        *best = (distance, surface);
    }
}

fn sphere(p: V, center: V, radius: f32) -> f32 {
    (p - center).len() - radius
}

/// Close enough to an ellipsoid's distance for marching and shading; it is
/// a bound, not the distance, which the march allows for.
fn ellipsoid(p: V, center: V, radii: V) -> f32 {
    let q = p - center;
    let k0 = V(q.0 / radii.0, q.1 / radii.1, q.2 / radii.2).len();
    let k1 = V(
        q.0 / (radii.0 * radii.0),
        q.1 / (radii.1 * radii.1),
        q.2 / (radii.2 * radii.2),
    )
    .len();
    if k1 < 1e-6 {
        return -radii.0.min(radii.1).min(radii.2);
    }
    k0 * (k0 - 1.) / k1
}

/// A capsule from `a` to `b` whose radius goes from `ra` to `rb`.
fn cone(p: V, a: V, b: V, ra: f32, rb: f32) -> f32 {
    let ba = b - a;
    let l2 = ba.dot(ba);
    let rr = ra - rb;
    let a2 = l2 - rr * rr;
    let il2 = 1. / l2;
    let pa = p - a;
    let y = pa.dot(ba);
    let z = y - l2;
    let across = pa * l2 - ba * y;
    let x2 = across.dot(across);
    let y2 = y * y * l2;
    let z2 = z * z * l2;
    let k = rr.signum() * rr * rr * x2;
    if z.signum() * a2 * z2 > k {
        return (x2 + z2).sqrt() * il2 - rb;
    }
    if y.signum() * a2 * y2 < k {
        return (x2 + y2).sqrt() * il2 - ra;
    }
    ((x2 * a2 * il2).sqrt() + y * rr) * il2 - ra
}

/// `cone`, pressed thin from front to back by `thin` (an ear, a fin).
fn blade(p: V, a: V, b: V, ra: f32, rb: f32, thin: f32) -> f32 {
    let press = |v: V| V(v.0, v.1, v.2 * thin);
    cone(press(p), press(a), press(b), ra, rb) / thin
}

/// An upright cylinder with softened rims.
fn cylinder(p: V, center: V, radius: f32, half_height: f32) -> f32 {
    let q = p - center;
    let around = (q.0 * q.0 + q.2 * q.2).sqrt() - radius;
    let along = q.1.abs() - half_height;
    let outside = (around.max(0.).powi(2) + along.max(0.).powi(2)).sqrt();
    outside + around.max(along).min(0.) - 0.012
}

/// A box with its edges rounded by `round`.
fn slab(p: V, center: V, half: V, round: f32) -> f32 {
    let q = p - center;
    let d = V(q.0.abs() - half.0, q.1.abs() - half.1, q.2.abs() - half.2);
    V(d.0.max(0.), d.1.max(0.), d.2.max(0.)).len() + d.0.max(d.1).max(d.2).min(0.) - round
}

/// A ring lying flat around the upright axis.
fn ring(p: V, center: V, radius: f32, thickness: f32) -> f32 {
    let q = p - center;
    let around = (q.0 * q.0 + q.2 * q.2).sqrt() - radius;
    (around * around + q.1 * q.1).sqrt() - thickness
}

/// Two shapes melted into one over `k`.
fn melt(a: f32, b: f32, k: f32) -> f32 {
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0., 1.);
    b + (a - b) * h - k * h * (1. - h)
}

/// The distance from `(x, y)` to the segment from `a` to `b`.
fn segment(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (px, py) = (x - a.0, y - a.1);
    let (bx, by) = (b.0 - a.0, b.1 - a.1);
    let h = ((px * bx + py * by) / (bx * bx + by * by)).clamp(0., 1.);
    ((px - bx * h).powi(2) + (py - by * h).powi(2)).sqrt()
}

struct Scene {
    look: Look,
    pose: Pose,
    body: Rgb,
    /// A small picture: larger eyes, and no mouth to blur into a smudge.
    bold: bool,
}

/// Where the camera looks from: a little above, so the head's top shows.
const TILT: f32 = 0.19;
const KEY_LIGHT: V = V(-0.50, 0.74, 0.60);
const FILL_LIGHT: V = V(0.80, 0.15, 0.55);

/// What `Scene::paint` leaves on the body.
struct Painted {
    color: Rgb,
    gloss: f32,
    /// Drawn as it is, whatever the light: a glint, a screen that glows.
    unlit: Option<Rgb>,
}

impl Scene {
    fn new(look: Look, pose: Pose, bold: bool) -> Self {
        let channel = |shift: u32| ((look.color >> shift) & 0xff) as f32 / 255.;
        let picked = [channel(16), channel(8), channel(0)];
        // The soft version of its colour that the pixel mascots wear, so a
        // bot keeps the colour it is known by.
        let body = pastel(picked);
        Self {
            look,
            pose,
            body,
            bold,
        }
    }

    /// Nothing is worn on the crown, so the species keeps what grows there.
    fn bare(&self) -> bool {
        !self.look.outfit.covers_top()
    }

    /// Where the top of the head is, for what sits on it.
    fn crown(&self) -> f32 {
        match self.look.species {
            Species::Mushroom => 0.80,
            // Flat, so a hat rests on it where a round head lets it sink in.
            Species::Robot => 0.71,
            _ => 0.66,
        }
    }

    /// The hand that carries what the role carries (a crab's is its claw),
    /// which way from it is away from the body, and how tall what it holds
    /// may stand: a mushroom's stays under the rim of its cap.
    fn hand(&self) -> (V, f32, f32) {
        let (hand, out, tall) = match self.look.species {
            Species::Crab => (V(0.82, 0.06, 0.20), -1., 1.),
            Species::Mushroom => (V(0.60, -0.34, 0.24), 1., 0.5),
            _ => (V(0.62, -0.26, 0.20), 1., 1.),
        };
        (hand + self.raised(), out, tall)
    }

    /// How far the hands are up: they rise with the open mouth, the frame a
    /// working bot alternates with its rest.
    fn raised(&self) -> V {
        match (self.pose, self.look.species) {
            (Pose::Talk, Species::Crab) => V(0., 0.14, 0.),
            (Pose::Talk, Species::Mushroom) => V(0.04, 0.12, 0.),
            (Pose::Talk, _) => V(0.05, 0.26, 0.),
            _ => V(0., 0., 0.),
        }
    }

    /// The nearest surface to `p` and what it is made of.
    fn nearest(&self, p: V) -> (f32, Surface) {
        let p = p - V(0., STAND, 0.);
        let m = p.mirrored();
        let species = self.look.species;
        let outfit = self.look.outfit;
        let bare = self.bare();
        let skin = Surface {
            color: self.body,
            gloss: 0.12,
            skin: true,
        };
        let dark = matter(mix(self.body, INK, 0.38), 0.2);

        // One body for every species: a head melted into a wider seat. A
        // robot's head is a rounded box.
        let head = match species {
            Species::Robot => slab(p, V(0., 0.12, 0.), V(0.40, 0.36, 0.36), 0.18),
            _ => sphere(p, V(0., 0.04, 0.), 0.62),
        };
        let mut body = melt(
            head,
            ellipsoid(p, V(0., -0.40, 0.), V(0.60, 0.40, 0.54)),
            0.22,
        );
        body = match species {
            // A ghost has no feet: its hem ends in a row of round lobes.
            Species::Ghost => {
                let lobes = sphere(m, V(0.15, -0.78, 0.10), 0.16).min(sphere(
                    m,
                    V(0.44, -0.72, 0.02),
                    0.15,
                ));
                melt(body, lobes, 0.10)
            }
            _ => melt(
                body,
                ellipsoid(m, V(0.25, -0.80, 0.14), V(0.17, 0.085, 0.21)),
                0.06,
            ),
        };
        body = match species {
            // A crab's hands are its claws: a short arm and a plump pincer,
            // notched from the top so it opens upward.
            Species::Crab => {
                let up = self.raised();
                let arm = cone(
                    m,
                    V(0.52, -0.30, 0.10),
                    V(0.72, -0.18, 0.14) + up,
                    0.10,
                    0.10,
                );
                let claw = ellipsoid(m, V(0.80, -0.04, 0.14) + up, V(0.17, 0.21, 0.14));
                let notch = cone(
                    m,
                    V(0.81, 0.04, 0.14) + up,
                    V(0.84, 0.30, 0.14) + up,
                    0.022,
                    0.085,
                );
                melt(body, melt(arm, claw, 0.05).max(-notch), 0.05)
            }
            _ => melt(
                body,
                sphere(m, V(0.60, -0.30, 0.16) + self.raised(), 0.115),
                0.07,
            ),
        };
        let mut best = (body, skin);

        match species {
            Species::Cat => {
                // Ears, not horns: wide at the root, thin front to back.
                let ear = blade(m, V(0.34, 0.44, 0.), V(0.46, 0.73, -0.02), 0.20, 0.085, 1.5);
                best.0 = melt(best.0, ear, 0.06);
            }
            Species::Robot => {
                if outfit != Outfit::Headset {
                    closer(
                        &mut best,
                        ellipsoid(m, V(0.60, 0.10, 0.), V(0.06, 0.13, 0.13)),
                        matter(STEEL, 0.9),
                    );
                }
                if bare {
                    closer(
                        &mut best,
                        cone(p, V(0., 0.62, 0.), V(0., 0.86, 0.), 0.032, 0.032),
                        matter(STEEL, 0.9),
                    );
                    closer(
                        &mut best,
                        sphere(p, V(0., 0.92, 0.), 0.085),
                        matter(RED, 1.),
                    );
                }
            }
            Species::Invader if bare => {
                closer(
                    &mut best,
                    cone(m, V(0.19, 0.58, 0.), V(0.29, 0.84, 0.), 0.045, 0.04),
                    dark,
                );
                closer(
                    &mut best,
                    sphere(m, V(0.30, 0.89, 0.), 0.085),
                    matter(GOLD, 0.9),
                );
            }
            Species::Frog if bare => {
                // The eyes sit up on these.
                let bump = sphere(m, V(0.27, 0.50, 0.12), 0.21);
                best.0 = melt(best.0, bump, 0.08);
            }
            Species::Dino => {
                let tail = cone(p, V(0.36, -0.62, -0.20), V(0.90, -0.42, -0.14), 0.20, 0.045);
                best.0 = melt(best.0, tail, 0.10);
                if bare {
                    // A low row of scutes, sunk half into the head.
                    let scutes =
                        sphere(m, V(0.20, 0.635, 0.), 0.09).min(sphere(p, V(0., 0.70, 0.), 0.10));
                    closer(&mut best, scutes, dark);
                }
            }
            Species::Mushroom => {
                let cap = ellipsoid(p, V(0., 0.40, 0.), V(0.86, 0.42, 0.80));
                // Open underneath, so the face looks out from under it.
                let cap = cap.max(-(p.1 - 0.16));
                let spots = [
                    V(-0.36, 0.66, 0.44),
                    V(0.32, 0.71, 0.40),
                    V(0.02, 0.50, 0.77),
                    V(-0.66, 0.40, 0.28),
                    V(0.66, 0.44, 0.24),
                ];
                let spotted = spots.iter().any(|spot| (p - *spot).len() < 0.115);
                let picked = if spotted {
                    [0.99, 0.97, 0.92]
                } else {
                    mix(self.body, INK, 0.08)
                };
                closer(&mut best, cap, matter(picked, 0.45));
            }
            Species::Rocket => {
                let fin = blade(
                    m,
                    V(0.50, -0.44, -0.06),
                    V(0.80, -0.80, -0.06),
                    0.14,
                    0.07,
                    2.,
                );
                closer(&mut best, fin, matter(RED, 0.5));
                if bare {
                    // A blunt nose, part of the hull.
                    let nose = cone(p, V(0., 0.40, 0.), V(0., 0.80, 0.), 0.40, 0.12);
                    best.0 = melt(best.0, nose, 0.18);
                    if best.1.skin && p.1 > 0.55 {
                        best.1.color = RED;
                    } else if best.1.skin && p.1 > 0.49 {
                        best.1.color = WHITE;
                    }
                }
            }
            _ => {}
        }
        // The mushroom's body under its cap is the pale stem.
        if species == Species::Mushroom && best.1.skin {
            best.1.color = [0.97, 0.94, 0.88];
        }

        let top = self.crown();
        let up = |y: f32| top + y;
        // A hat leans about the middle of the crown it sits on.
        let hp = p.leaned(V(0., top - 0.12, 0.), outfit.lean());
        match outfit {
            Outfit::Plain | Outfit::Glasses => {}
            Outfit::Crown => {
                closer(
                    &mut best,
                    ring(hp, V(0., up(0.0), 0.), 0.29, 0.055),
                    matter(GOLD, 1.),
                );
                for spike in 0..5 {
                    let angle = spike as f32 * std::f32::consts::TAU / 5. + 0.63;
                    let at = V(angle.sin() * 0.29, up(0.03), angle.cos() * 0.29);
                    closer(
                        &mut best,
                        cone(hp, at, at + V(0., 0.20, 0.), 0.062, 0.028),
                        matter(GOLD, 1.),
                    );
                }
                closer(
                    &mut best,
                    sphere(hp, V(0., up(0.02), 0.33), 0.05),
                    matter([0.93, 0.27, 0.36], 1.),
                );
            }
            Outfit::Headset => {
                // The band: the top half of a ring standing up over the head.
                let q = p - V(0., 0.04, 0.);
                let reach = if species == Species::Robot {
                    0.79
                } else {
                    0.685
                };
                let around = (q.0 * q.0 + q.1 * q.1).sqrt() - reach;
                let band = (around * around + q.2 * q.2).sqrt() - 0.045;
                closer(&mut best, band.max(-(q.1 - 0.05)), matter(CHARCOAL, 0.6));
                closer(
                    &mut best,
                    ellipsoid(m, V(0.66, 0.02, 0.), V(0.10, 0.18, 0.18)),
                    matter(CHARCOAL, 0.6),
                );
                closer(
                    &mut best,
                    cone(p, V(-0.66, -0.06, 0.08), V(-0.34, -0.27, 0.56), 0.03, 0.03),
                    matter(CHARCOAL, 0.6),
                );
                closer(
                    &mut best,
                    sphere(p, V(-0.32, -0.28, 0.58), 0.065),
                    matter([0.36, 0.72, 0.98], 0.9),
                );
            }
            Outfit::PostCap => {
                let blue = [0.25, 0.44, 0.86];
                let dome = ellipsoid(hp, V(0., up(-0.03), 0.), V(0.48, 0.23, 0.46));
                closer(&mut best, dome.max(-(hp.1 - up(-0.12))), matter(blue, 0.3));
                closer(
                    &mut best,
                    ellipsoid(hp, V(0., up(-0.10), 0.42), V(0.31, 0.05, 0.22)),
                    matter(mix(blue, INK, 0.45), 0.7),
                );
                closer(
                    &mut best,
                    sphere(hp, V(0., up(-0.01), 0.45), 0.055),
                    matter(GOLD, 1.),
                );
            }
            Outfit::TopHat => {
                closer(
                    &mut best,
                    cylinder(hp, V(0., up(-0.02), 0.), 0.47, 0.022),
                    matter(CHARCOAL, 0.5),
                );
                let band = (hp.1 - up(0.05)).abs() < 0.05;
                let color = if band { [0.20, 0.72, 0.50] } else { CHARCOAL };
                closer(
                    &mut best,
                    cylinder(hp, V(0., up(0.17), 0.), 0.30, 0.19),
                    matter(color, 0.5),
                );
            }
            Outfit::Mortarboard => {
                let navy = [0.18, 0.22, 0.42];
                closer(
                    &mut best,
                    ellipsoid(hp, V(0., up(-0.05), 0.), V(0.37, 0.18, 0.37)),
                    matter(navy, 0.4),
                );
                // The board, turned a corner toward the front.
                let turn = std::f32::consts::FRAC_1_SQRT_2;
                let turned = V((hp.0 - hp.2) * turn, hp.1, (hp.0 + hp.2) * turn);
                closer(
                    &mut best,
                    slab(turned, V(0., up(0.10), 0.), V(0.40, 0.022, 0.40), 0.012),
                    matter(navy, 0.4),
                );
                closer(
                    &mut best,
                    sphere(hp, V(0., up(0.14), 0.), 0.045),
                    matter(GOLD, 0.9),
                );
                closer(
                    &mut best,
                    cone(
                        hp,
                        V(0., up(0.13), 0.),
                        V(0.40, up(-0.10), 0.30),
                        0.018,
                        0.032,
                    ),
                    matter(GOLD, 0.6),
                );
            }
            Outfit::Explorer => {
                let khaki = [0.82, 0.70, 0.47];
                closer(
                    &mut best,
                    cylinder(hp, V(0., up(-0.05), 0.), 0.63, 0.018),
                    matter(khaki, 0.2),
                );
                let band = (hp.1 - up(0.02)).abs() < 0.04;
                let color = if band { mix(khaki, INK, 0.55) } else { khaki };
                closer(
                    &mut best,
                    ellipsoid(hp, V(0., up(0.0), 0.), V(0.40, 0.27, 0.40)),
                    matter(color, 0.2),
                );
            }
            Outfit::Bow => {
                let rests = if species == Species::Mushroom {
                    0.78
                } else {
                    0.56
                };
                let centre = V(0.25, rests, 0.24);
                let q = p - centre;
                let wing = ellipsoid(
                    V(q.0.abs(), q.1, q.2),
                    V(0.15, 0., 0.),
                    V(0.16, 0.115, 0.07),
                );
                closer(&mut best, wing, matter(PINK, 0.5));
                closer(
                    &mut best,
                    sphere(p, centre, 0.07),
                    matter(mix(PINK, INK, 0.18), 0.5),
                );
            }
            Outfit::Beret => {
                // Soft and wide, flopping to one side, with its little stalk.
                let felt = [0.86, 0.29, 0.36];
                let cap = melt(
                    ellipsoid(hp, V(0.03, up(-0.03), 0.), V(0.52, 0.15, 0.48)),
                    ellipsoid(hp, V(0.14, up(0.03), 0.), V(0.46, 0.13, 0.42)),
                    0.10,
                );
                closer(&mut best, cap, matter(felt, 0.2));
                closer(
                    &mut best,
                    cone(hp, V(0.12, up(0.13), 0.), V(0.14, up(0.21), 0.), 0.03, 0.03),
                    matter(mix(felt, INK, 0.3), 0.2),
                );
            }
            Outfit::Captain => {
                // A white peaked cap: a flat top wider than its navy band,
                // a short black visor in front and a gold badge on the band.
                let navy = [0.13, 0.17, 0.34];
                let top = melt(
                    cylinder(hp, V(0., up(0.06), 0.), 0.33, 0.07),
                    ellipsoid(hp, V(0., up(0.14), 0.), V(0.47, 0.08, 0.45)),
                    0.05,
                );
                closer(&mut best, top, matter([0.97, 0.97, 0.95], 0.35));
                closer(
                    &mut best,
                    cylinder(hp, V(0., up(-0.03), 0.), 0.345, 0.05),
                    matter(navy, 0.4),
                );
                let visor = ellipsoid(hp, V(0., up(-0.08), 0.34), V(0.29, 0.035, 0.20));
                closer(&mut best, visor.max(-(hp.2 - 0.22)), matter(INK, 0.9));
                closer(
                    &mut best,
                    ellipsoid(hp, V(0., up(-0.01), 0.35), V(0.07, 0.05, 0.03)),
                    matter(GOLD, 1.),
                );
            }
            Outfit::HardHat => {
                // A safety-yellow shell with a ridge down the middle and a
                // thin brim all round.
                let yellow = [1.0, 0.80, 0.10];
                let shell = melt(
                    ellipsoid(hp, V(0., up(-0.01), 0.), V(0.45, 0.29, 0.45)),
                    ellipsoid(hp, V(0., up(0.0), 0.), V(0.07, 0.32, 0.43)),
                    0.04,
                );
                closer(
                    &mut best,
                    shell.max(-(hp.1 - up(-0.06))),
                    matter(yellow, 0.75),
                );
                closer(
                    &mut best,
                    cylinder(hp, V(0., up(-0.06), 0.04), 0.54, 0.016),
                    matter(mix(yellow, INK, 0.12), 0.75),
                );
            }
            Outfit::Flag => {
                let (hand, out, tall) = self.hand();
                closer(
                    &mut best,
                    cone(
                        p,
                        hand + V(0., -0.16, 0.),
                        hand + V(0.02 * out, 0.72 * tall, 0.),
                        0.028,
                        0.028,
                    ),
                    matter(STEEL, 0.8),
                );
                closer(
                    &mut best,
                    slab(
                        p,
                        hand + V(0.18 * out, 0.72 * tall - 0.12, 0.),
                        V(0.15, 0.10, 0.012),
                        0.012,
                    ),
                    matter(RED, 0.4),
                );
            }
            Outfit::Quill => {
                // A feather held by its stem: wide in the middle, a fine tip.
                let (hand, out, tall) = self.hand();
                let stem = hand + V(0., -0.10, 0.);
                let belly = hand + V(0.13 * out, 0.30 * tall, -0.02);
                let tip = hand + V(0.22 * out, 0.62 * tall, -0.04);
                let feather =
                    cone(p, stem, belly, 0.022, 0.10).min(cone(p, belly, tip, 0.10, 0.02));
                closer(&mut best, feather, matter([0.98, 0.96, 0.90], 0.25));
            }
            Outfit::Phone => {
                let (hand, out, _) = self.hand();
                let centre = hand + V(0.03 * out, 0.12, 0.07);
                let q = p.leaned(centre, -0.18 * out);
                let lit = (q.0 - centre.0).abs() < 0.066
                    && (q.1 - centre.1).abs() < 0.125
                    && q.2 > centre.2;
                let color = if lit { [0.55, 0.80, 1.0] } else { CHARCOAL };
                closer(
                    &mut best,
                    slab(q, centre, V(0.075, 0.14, 0.012), 0.022),
                    matter(color, 0.9),
                );
            }
        }
        best
    }

    fn distance(&self, p: V) -> f32 {
        self.nearest(p).0
    }

    fn normal(&self, p: V) -> V {
        const E: f32 = 0.0025;
        let k = [
            V(1., -1., -1.),
            V(-1., -1., 1.),
            V(-1., 1., -1.),
            V(1., 1., 1.),
        ];
        k.iter()
            .fold(V(0., 0., 0.), |sum, k| sum + *k * self.distance(p + *k * E))
            .unit()
    }

    /// The colour and cover of the picture at `x`, `y` (each −VIEW to VIEW,
    /// `y` up).
    fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        let (sin, cos) = TILT.sin_cos();
        let toward = V(0., -sin, -cos);
        let origin = V(x, y * cos + 3. * sin, 3. * cos - y * sin);
        let mut travelled = 1.2;
        for _ in 0..80 {
            let p = origin + toward * travelled;
            let (distance, surface) = self.nearest(p);
            if distance < 0.0018 {
                let [r, g, b] = self.shade(p, surface, toward);
                return [r, g, b, 1.];
            }
            // Short of the full distance: melted and pressed shapes report
            // a bound, and a full step could pass through a thin one.
            travelled += distance * 0.8;
            if travelled > 5. {
                break;
            }
        }
        // Nothing hit: the soft shadow it stands on, or clear.
        let (dx, dy) = (x / 0.62, (y - (STAND - 0.93)) / 0.085);
        let shadow = 0.26 * (1. - smoothstep(0.35, 1., (dx * dx + dy * dy).sqrt()));
        [0.05, 0.06, 0.09, shadow]
    }

    fn shade(&self, p: V, surface: Surface, toward: V) -> Rgb {
        let normal = self.normal(p);
        let eye = toward * -1.;
        let mut color = surface.color;
        let mut gloss = surface.gloss;
        if surface.skin {
            let painted = self.paint(p - V(0., STAND, 0.), normal, color, gloss);
            if let Some(unlit) = painted.unlit {
                return unlit;
            }
            (color, gloss) = (painted.color, painted.gloss);
        }
        let light = KEY_LIGHT.unit();
        // Wrapped, so the side away from the light stays soft, not black.
        let wrap = ((normal.dot(light) + 0.30) / 1.30).clamp(0., 1.);
        // Darker where another surface is close: under a hat, between feet.
        let near = (self.distance(p + normal * 0.11) / 0.11).clamp(0., 1.);
        // Four tones of the one colour, as a soft toy takes the light: its
        // shadows deepen in colour instead of going grey.
        let grey = (color[0] + color[1] + color[2]) / 3.;
        let deep = color.map(|c| ((grey + (c - grey) * 1.30) * 0.50).max(0.));
        let shade = color.map(|c| ((grey + (c - grey) * 1.15) * 0.76).max(0.));
        let pale = mix(color, WHITE, 0.40);
        let t = wrap.powf(1.15);
        let mut lit = if t < 0.4 {
            mix(deep, shade, smoothstep(0., 0.4, t))
        } else if t < 0.72 {
            mix(shade, color, smoothstep(0.4, 0.72, t))
        } else {
            mix(color, pale, smoothstep(0.72, 1., t))
        };
        let fill = normal.dot(FILL_LIGHT.unit()).max(0.) * 0.10;
        let bounce = (-normal.1).clamp(0., 1.) * 0.14 * near;
        // Less of it on a small picture, where it would wash the colour out.
        let sheen = (1. - normal.dot(eye).max(0.)).powf(2.4) * if self.bold { 0.10 } else { 0.24 };
        let half = (light + eye).unit();
        let shine = normal.dot(half).max(0.).powf(14. + 90. * gloss) * (0.05 + 0.85 * gloss);
        let occluded = 0.80 + 0.20 * near;
        for (channel, value) in lit.iter_mut().enumerate() {
            *value = *value * occluded
                + color[channel] * fill
                + pale[channel] * (bounce + sheen)
                + shine;
        }
        lit
    }

    /// The face and the tummy, painted where the body faces the front.
    fn paint(&self, p: V, normal: V, color: Rgb, gloss: f32) -> Painted {
        let mut painted = Painted {
            color,
            gloss,
            unlit: None,
        };
        if normal.2 < 0.12 {
            return painted;
        }
        let species = self.look.species;
        let ax = p.0.abs();
        let face_y = match species {
            Species::Mushroom => -0.14,
            Species::Robot => 0.11,
            _ => 0.04,
        };
        // A frog's eyes sit up on its bumps while its head is bare.
        let (eye_x, eye_y) = match species {
            Species::Frog if self.bare() => (0.27, 0.54),
            Species::Skull => (0.21, face_y),
            _ => (0.19, face_y),
        };
        let mouth_y = face_y - 0.125;

        // A paler tummy.
        let (tx, ty) = (p.0 / 0.31, (p.1 + 0.50) / 0.24);
        let tummy = 1. - smoothstep(0.75, 1., (tx * tx + ty * ty).sqrt());
        if !self.bold {
            painted.color = mix(painted.color, WHITE, 0.30 * tummy);
        }

        if species == Species::Cat && p.1 > 0.46 {
            // The pink inside of each ear.
            let along = segment(ax, p.1, (0.35, 0.50), (0.44, 0.69));
            let reach = 0.09 - 0.035 * smoothstep(0.50, 0.70, p.1);
            if along < reach {
                painted.color = mix(painted.color, PINK, 0.85);
            }
            return painted;
        }

        if species == Species::Robot {
            return self.screen(p, face_y, painted);
        }

        // Cheeks, low and outside the eyes.
        if !self.bold {
            let (cx, cy) = ((ax - (eye_x + 0.15)) / 0.10, (p.1 - (face_y - 0.10)) / 0.06);
            let cheek = 1. - smoothstep(0.3, 1., (cx * cx + cy * cy).sqrt());
            painted.color = mix(painted.color, [1.0, 0.55, 0.60], 0.34 * cheek);
        }

        // Eyes: tall beads that catch the light; a skull's are sockets.
        let grown = if self.bold { 1.45 } else { 1. };
        let (wide, tall) = match species {
            Species::Skull => (0.115 * grown, 0.135 * grown),
            _ => (0.078 * grown, 0.108 * grown),
        };
        let (ex, ey) = (ax - eye_x, p.1 - eye_y);
        let (nx, ny) = (ex / wide, ey / tall);
        match self.pose {
            Pose::Blink => {
                // Shut: an arc, as eyes close when they smile.
                let (dx, dy) = (ex, ey + wide * 1.0);
                let arc = ((dx * dx + dy * dy).sqrt() - wide * 1.05).abs();
                if arc < 0.017 && dy > wide * 0.35 {
                    painted.color = INK;
                }
            }
            _ if nx * nx + ny * ny < 1. => {
                // The glint sits toward the light, on the same side of both.
                let side = p.0.signum();
                let (gx, gy) = ((p.0 - side * eye_x) / wide + 0.16, ny - 0.46);
                let (gx, gy) = (gx * 0.96 + gy * 0.28, -gx * 0.28 + gy * 0.96);
                let glint = (gx / 0.48).powi(2) + (gy / 0.30).powi(2) < 1.;
                let dot =
                    (((p.0 - side * eye_x) / wide - 0.30).powi(2) + (ny + 0.50).powi(2)) < 0.035;
                painted.unlit = Some(if glint && species != Species::Skull {
                    WHITE
                } else if species == Species::Skull {
                    // A hollow socket with a small light in it.
                    if (nx * nx + ny * ny) < 0.09 {
                        [0.92, 0.94, 1.0]
                    } else {
                        [0.09, 0.09, 0.12]
                    }
                } else if dot {
                    [0.78, 0.86, 1.0]
                } else {
                    // Near black, with the sky's blue coming up from below.
                    let sky = smoothstep(0.10, -0.80, ny) * 0.85;
                    mix([0.03, 0.03, 0.05], [0.36, 0.50, 0.84], sky)
                });
                return painted;
            }
            _ => {}
        }
        if self.look.outfit == Outfit::Glasses {
            let rim = ((ex / 0.155).powi(2) + (ey / 0.155).powi(2)).sqrt();
            let bridge = ax < eye_x - 0.14 && (ey - 0.02).abs() < 0.017;
            if (rim - 1.).abs() < 0.14 || bridge {
                painted.color = CHARCOAL;
                painted.gloss = 0.9;
                return painted;
            }
        }

        // The mouth, small and close under the eyes.
        if self.pose == Pose::Talk {
            let inside = (p.0 / 0.055).powi(2) + ((p.1 - mouth_y + 0.02) / 0.068).powi(2);
            if inside < 1. {
                painted.color = if p.1 < mouth_y - 0.045 {
                    [0.95, 0.47, 0.52]
                } else {
                    [0.36, 0.10, 0.15]
                };
                painted.gloss = 0.2;
            }
            return painted;
        }
        if self.bold {
            return painted;
        }
        let ink = match species {
            Species::Skull => {
                // A nose hole and a row of teeth.
                let nose = (p.1 - (face_y - 0.15)).abs() < 0.035
                    && ax < 0.03 - (p.1 - (face_y - 0.185)).max(0.) * 0.5;
                let row = mouth_y - 0.14;
                let teeth = ax < 0.15 && (p.1 - row).abs() < 0.045;
                if teeth {
                    let gap = ((p.0 + 0.15) / 0.075).fract();
                    let edge = !(0.14..=0.86).contains(&gap) || (p.1 - row).abs() > 0.034;
                    if !edge {
                        painted.color = WHITE;
                    }
                    edge
                } else {
                    nose
                }
            }
            Species::Cat => {
                // «ω»: two small arcs under a pink nose.
                let (dx, dy) = (ax - 0.042, p.1 - (mouth_y + 0.035));
                let arc = ((dx * dx + dy * dy).sqrt() - 0.042).abs();
                let (nx, ny) = (p.0 / 0.032, (p.1 - (mouth_y + 0.05)) / 0.022);
                if nx * nx + ny * ny < 1. {
                    painted.color = PINK;
                }
                arc < 0.014 && dy < -0.008
            }
            Species::Invader => {
                // A zigzag, as its pixel self grins.
                let wave = ((p.0 / 0.07 + 0.5).fract() - 0.5).abs() * 0.07;
                ax < 0.125 && (p.1 - (mouth_y - 0.01) - wave).abs() < 0.014
            }
            Species::Frog => {
                // Wide, from cheek to cheek.
                let (dx, dy) = (p.0, p.1 - (mouth_y + 0.36));
                let arc = ((dx * dx + dy * dy).sqrt() - 0.37).abs();
                arc < 0.015 && ax < 0.21 && dy < 0.
            }
            _ => {
                let nostril = species == Species::Dino
                    && ((ax - 0.035).powi(2) + (p.1 - (mouth_y + 0.055)).powi(2)) < 0.00016;
                // A small smile: the lower arc of a circle.
                let (dx, dy) = (p.0, p.1 - (mouth_y + 0.055));
                let arc = ((dx * dx + dy * dy).sqrt() - 0.062).abs();
                nostril || (arc < 0.015 && dy < -0.030)
            }
        };
        if ink {
            painted.color = INK;
        }
        painted
    }

    /// A robot's face: a dark screen with eyes that glow.
    fn screen(&self, p: V, face_y: f32, mut painted: Painted) -> Painted {
        let (sx, sy) = (p.0 / 0.42, (p.1 - face_y) / 0.25);
        let inside = (sx.powi(4) + sy.powi(4)).powf(0.25);
        if inside > 1. {
            // The bezel around the screen.
            if inside < 1.10 {
                painted.color = mix(painted.color, STEEL, 0.75);
                painted.gloss = 0.7;
            }
            return painted;
        }
        let glass = [0.07, 0.09, 0.15];
        let glow = [0.55, 0.95, 1.0];
        let grown = if self.bold { 1.3 } else { 1. };
        let (ex, ey) = (p.0.abs() - 0.19, p.1 - (face_y + 0.03));
        let eye = match self.pose {
            Pose::Blink => ex.abs() < 0.075 * grown && ey.abs() < 0.016,
            _ => {
                let (nx, ny) = (ex / (0.068 * grown), ey / (0.085 * grown));
                (nx.powi(4) + ny.powi(4)) < 1.
            }
        };
        let mouth_y = face_y - 0.13;
        let mouth = match self.pose {
            Pose::Talk => p.0.abs() < 0.06 && (p.1 - mouth_y).abs() < 0.04,
            _ if self.bold => false,
            _ => {
                let (dx, dy) = (p.0, p.1 - (mouth_y + 0.06));
                let arc = ((dx * dx + dy * dy).sqrt() - 0.07).abs();
                arc < 0.015 && dy < -0.035
            }
        };
        let rim = ((ex / 0.15).powi(2) + (ey / 0.15).powi(2)).sqrt();
        let bridge = p.0.abs() < 0.05 && (ey - 0.02).abs() < 0.016;
        let glasses = self.look.outfit == Outfit::Glasses && ((rim - 1.).abs() < 0.13 || bridge);
        painted.unlit = Some(if eye || mouth {
            glow
        } else if glasses {
            [0.86, 0.89, 0.96]
        } else {
            // A faint sheen across the glass, brighter toward the light.
            mix(
                glass,
                [0.20, 0.26, 0.38],
                smoothstep(-0.2, 1.2, sy - sx) * 0.6,
            )
        });
        painted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(mascot: &str, role: &str) -> Look {
        let (species, outfit) = parse(&key(mascot, Some(role))).unwrap();
        Look {
            species,
            outfit,
            color: 0x38bdf8,
        }
    }

    fn alpha(picture: &[u8], side: u32, x: u32, y: u32) -> u8 {
        picture[((y * side + x) * 4 + 3) as usize]
    }

    /// Opaque pixels in the picture's rows from `rows.start` to `rows.end`.
    fn covered(picture: &[u8], side: u32, rows: std::ops::Range<u32>) -> usize {
        rows.flat_map(|y| (0..side).map(move |x| (x, y)))
            .filter(|(x, y)| alpha(picture, side, *x, *y) > 200)
            .count()
    }

    #[test]
    fn a_face_names_its_species_and_outfit_and_a_mascot_names_neither() {
        assert_eq!(key("cat", Some("writer")), "3d:cat:writer");
        assert_eq!(key("cat", None), "3d:cat:custom");
        assert_eq!(parse("3d:cat:writer"), Some((Species::Cat, Outfit::Quill)));
        // A role or mascot from a newer version still draws.
        assert_eq!(
            parse("3d:dragon:pilot"),
            Some((Species::Ghost, Outfit::Plain))
        );
        assert_eq!(parse("cat"), None);
        assert_eq!(mascot("3d:cat:writer"), "cat");
        assert_eq!(mascot("cat"), "cat");
    }

    #[test]
    fn the_character_fills_the_middle_and_leaves_the_corners_clear() {
        let side = 48;
        let picture = render(look("ghost", "custom"), Pose::Rest, side);
        assert_eq!(picture.len(), (side * side * 4) as usize);
        assert_eq!(alpha(&picture, side, side / 2, side / 2), 255);
        for (x, y) in [(0, 0), (side - 1, 0), (0, side - 1), (side - 1, side - 1)] {
            assert_eq!(alpha(&picture, side, x, y), 0);
        }
        // Drawn the same every time, so a pose is rendered once and kept.
        assert_eq!(picture, render(look("ghost", "custom"), Pose::Rest, side));
    }

    /// The pixels that differ between two pictures, as `(x, y)`.
    fn changed(a: &[u8], b: &[u8], side: u32) -> Vec<(u32, u32)> {
        a.as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(index, _)| (index as u32 % side, index as u32 / side))
            .collect()
    }

    #[test]
    fn a_blink_changes_only_the_eyes_and_work_lifts_the_hands() {
        let side = 64;
        for mascot in ["cat", "robot", "frog", "crab"] {
            let rest = render(look(mascot, "custom"), Pose::Rest, side);
            let blink = changed(
                &rest,
                &render(look(mascot, "custom"), Pose::Blink, side),
                side,
            );
            assert!(!blink.is_empty(), "{mascot}");
            // Within the head: never the picture's edge or its feet.
            for (x, y) in blink {
                assert!((side / 5..side * 4 / 5).contains(&x), "{mascot} {x},{y}");
                assert!((side / 8..side * 3 / 4).contains(&y), "{mascot} {x},{y}");
            }
            // Working, its mouth opens and its hands go up: the outline
            // changes at its sides, as a pixel mascot's does.
            let talk = changed(
                &rest,
                &render(look(mascot, "custom"), Pose::Talk, side),
                side,
            );
            assert!(talk.iter().any(|(x, _)| *x < side / 5), "{mascot}");
            assert!(talk.iter().any(|(x, _)| *x > side * 4 / 5), "{mascot}");
            assert!(
                talk.iter()
                    .any(|(x, _)| (side * 2 / 5..side * 3 / 5).contains(x)),
                "{mascot}"
            );
            assert!(talk.iter().all(|(_, y)| *y > side / 6), "{mascot}");
        }
    }

    #[test]
    fn a_small_picture_is_a_portrait_in_the_bots_own_colour() {
        // Large, the whole character stands clear of the picture's edge.
        let large = render(look("ghost", "custom"), Pose::Rest, 64);
        assert_eq!(covered(&large, 64, 63..64), 0);
        // Small, the head and shoulders fill it and the frame cuts the rest.
        let small = render(look("ghost", "custom"), Pose::Rest, 16);
        assert!(covered(&small, 16, 15..16) > 4);
        assert!(covered(&small, 16, 0..16) > covered(&large, 64, 0..64) / 16);
        // The body is the pixel mascots' pastel of the colour picked: for
        // this blue, lighter and still blue.
        let [r, g, b] = pastel([0.22, 0.74, 0.97]);
        assert!(b > g && g > r && r > 0.45 && b < 0.95, "{r} {g} {b}");
        assert_eq!(pastel([0.5, 0.5, 0.5]), [0.65, 0.65, 0.65]);
        // A clear pixel carries that colour under its zero alpha.
        assert_eq!(small[3], 0);
        assert!(small[2] > small[0]);
        // A portrait can have more pixels, for a dense screen: still the
        // head and shoulders, cut by the frame.
        let sharp = render_at(look("ghost", "custom"), Pose::Rest, 64, true);
        assert!(covered(&sharp, 64, 63..64) > 16);
        assert_eq!(
            render_at(look("ghost", "custom"), Pose::Rest, 16, true),
            small
        );
        assert_eq!(
            render_at(look("ghost", "custom"), Pose::Rest, 64, false),
            large
        );
    }

    #[test]
    fn a_hat_takes_the_place_of_what_grows_on_the_head() {
        let side = 64;
        // The band above the head, where antennae, a crest or a nose stand.
        let above = 0..side / 5;
        let ghost = render(look("ghost", "custom"), Pose::Rest, side);
        assert_eq!(covered(&ghost, side, above.clone()), 0);
        for mascot in ["invader", "rocket", "dino", "frog"] {
            let bare = render(look(mascot, "custom"), Pose::Rest, side);
            // A frog's bumps stay low; the others stand clear of the head.
            if mascot != "frog" {
                assert!(
                    covered(&bare, side, above.clone()) > 0,
                    "{mascot} grows something on a bare head"
                );
            }
            // Under a top hat every species shows the same hat and no more:
            // nothing pokes through it or out beside it.
            let hatted = render(look(mascot, "finance"), Pose::Rest, side);
            let plain = render(look("ghost", "finance"), Pose::Rest, side);
            assert_eq!(
                covered(&hatted, side, above.clone()),
                covered(&plain, side, above.clone()),
                "{mascot} under a hat"
            );
        }
        // A robot's hat rests higher, on its flat head, and its antenna with
        // the red ball is gone under it all the same.
        let red = |picture: &[u8]| {
            picture
                .as_chunks::<4>()
                .0
                .iter()
                .take((side * side / 5) as usize)
                .filter(|px| px[3] > 200 && px[0] > 200 && px[1] < 130 && px[2] < 130)
                .count()
        };
        assert!(red(&render(look("robot", "custom"), Pose::Rest, side)) > 0);
        assert_eq!(red(&render(look("robot", "finance"), Pose::Rest, side)), 0);
        // What a role carries is in the hand: it adds nothing above the head.
        let bare = render(look("ghost", "custom"), Pose::Rest, side);
        for role in ["planner", "writer", "phone"] {
            let carrying = render(look("ghost", role), Pose::Rest, side);
            assert_eq!(
                covered(&carrying, side, 0..side / 8),
                covered(&bare, side, 0..side / 8),
                "{role}"
            );
            assert!(covered(&carrying, side, 0..side) > covered(&bare, side, 0..side));
        }
    }
}
