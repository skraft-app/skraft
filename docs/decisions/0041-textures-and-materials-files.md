# Textures by box projection, materials alone in a .skraftmat

- Status: accepted (2026-11-19)
- Design: [Textures](../render/look.md#textures), [File format](../io/file-format.md#materials-alone-skraftmat)

## Context

Materials were a name and a colour. They are to show their wood (or their panel) and to be
exported alone, to build libraries, « without reinventing the wheel ». Decided on 2026-11-19
(questions / answers): the images in the ZIP by their hash, as they came; a material export is a
document without geometry in the same container, extension `.skraftmat`; SketchUp's `.skm`
import later, no PBR standard (glTF, MaterialX) for now.

## Decision

- `Material.texture: Option<Texture { image, size, rotation }>`; the images in
  `Document.assets` by the hash of their bytes, `textures/<id>.<ext>` in the file, stored once,
  dropped when no material uses them.
- **Mapped by a box projection in the part's axes**, in the shader: the image's width along
  the part's length (its mesh's longest axis), at its real size; no UVs.
- **Rendered from one `texture_2d_array`**, every image at 1024² texels with its mip levels
  made on the CPU; each palette entry's texture beside its colour; the mip level from the size
  of a pixel on the face (`textureSampleLevel`).
- **A `.skraftmat` is a `.skraft` of `content: "materials"`**: a document without geometry,
  imported by a graft (`ImportMaterials`), its own extension and file associations.

## Alternatives considered

- **UVs in the meshes** (the kernel's tessellation, or computed per face): every mesh larger,
  re-uploaded with every texture change; a box projection gives what a board shows.
- **A texture per material, bound per draw**: a draw per material instead of one per
  definition, against the instancing ([0011](0011-per-occurrence-costs-instances-and-indexes.md)).
- **Mip levels by the GPU's derivatives** (`textureSample`): needs a uniform control flow,
  broken by the section's `discard` and the per-material branch.
- **A format of its own for materials, or an existing one** (SketchUp's `.skm`, MaterialX,
  glTF): a second reader and writer for what a document already holds; `.skm` will be read
  later as an import.

## Consequences

- One image size for all: a fine wood shows its 1024 texels however large it is laid.
- The images are decoded on the UI thread when first shown (the viewport keeps them decoded,
  by id, for the session).
- Section caps and back faces keep their colours.
