//! What an export writes, whatever its format (docs/io/export.md): products — a part or an
//! assembly each — their bodies by the job's inputs, their placements and colours. The model
//! gathers them; the STEP and SketchUp writers take them (`step_export.rs`, `skp_export.rs`).

use glam::DAffine3;
use serde::{Deserialize, Serialize};

/// A part or an assembly: its own bodies and the products it places.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExportProduct {
    pub name: String,
    pub bodies: Vec<ExportBody>,
    /// The products it places: an index into its export's products, the placement in this
    /// product's axes (rigid for STEP; a mirror for SketchUp), the occurrence's name.
    pub children: Vec<(usize, DAffine3, String)>,
}

/// A body of a product, in its axes (mm).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportBody {
    pub name: String,
    /// The job's input holding its record.
    pub solid: u32,
    /// Written mirrored across its YZ plane (x → −x): a mirrored occurrence's twin (STEP).
    pub mirrored: bool,
    /// The whole body's colour.
    pub color: Option<Paint>,
    /// Faces with colours of their own: index in the record's faces, their colour.
    pub faces: Vec<(u32, Paint)>,
}

/// A colour as it goes out: what it is named after (a material, a finish over it), its sRGB,
/// and the texture it shows (a SketchUp export's: STEP has colours only).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paint {
    pub name: String,
    pub rgb: [u8; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<PaintTexture>,
}

/// A paint's texture: its image among its export's, and its real size (mm: the image's width,
/// then its height).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaintTexture {
    pub image: u32,
    pub size: [f64; 2],
}

impl Paint {
    pub fn new(name: &str, rgb: [u8; 3]) -> Self {
        Paint { name: name.to_owned(), rgb, texture: None }
    }

    /// What tells two paints apart: their name, colour, image and size.
    pub(crate) fn key(&self) -> PaintKey {
        (self.name.clone(), self.rgb, self.texture.map(|t| (t.image, t.size.map(f64::to_bits))))
    }
}

/// [`Paint::key`]: a paint's name, colour, image and size (its bits).
pub(crate) type PaintKey = (String, [u8; 3], Option<(u32, [u64; 2])>);

/// The mirror a twin's records are written through, in the component's axes.
pub const MIRROR: DAffine3 =
    DAffine3 { matrix3: glam::DMat3::from_cols(glam::DVec3::NEG_X, glam::DVec3::Y, glam::DVec3::Z), translation: glam::DVec3::ZERO };
