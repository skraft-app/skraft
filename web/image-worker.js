// Decodes the textures' images off the page's thread (docs/render/look.md#textures), with the
// browser's own decoders. In: { key, bytes, exact } (a square of that side) or { key, bytes, fit }
// (its proportions kept within that side). Out: { key, width, height, rgba }, or { key } when
// the image does not read: the UI then decodes it itself.
self.onmessage = async ({ data: { key, bytes, exact, fit } }) => {
  try {
    const bitmap = await createImageBitmap(new Blob([bytes]), { colorSpaceConversion: "none" });
    const scale = fit ? Math.min(1, fit / Math.max(bitmap.width, bitmap.height)) : 1;
    const width = exact ?? Math.max(1, Math.round(bitmap.width * scale));
    const height = exact ?? Math.max(1, Math.round(bitmap.height * scale));
    const canvas = new OffscreenCanvas(width, height);
    const context = canvas.getContext("2d", { willReadFrequently: true, colorSpace: "srgb" });
    context.imageSmoothingQuality = "high";
    context.drawImage(bitmap, 0, 0, width, height);
    bitmap.close();
    const rgba = context.getImageData(0, 0, width, height).data.buffer;
    self.postMessage({ key, width, height, rgba }, [rgba]);
  } catch {
    self.postMessage({ key });
  }
};
