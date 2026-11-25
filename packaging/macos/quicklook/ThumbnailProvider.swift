// Finder's thumbnail of a .skraft file: the picture it carries, fitted in the size asked
// (FilePicture.swift). A file without one keeps the document icon.

import AppKit
import QuickLookThumbnailing

final class ThumbnailProvider: QLThumbnailProvider {
    override func provideThumbnail(for request: QLFileThumbnailRequest, _ handler: @escaping (QLThumbnailReply?, Error?) -> Void) {
        guard let png = FilePicture.png(at: request.fileURL), let image = NSImage(data: png),
              image.size.width > 0, image.size.height > 0 else {
            handler(nil, nil)
            return
        }
        let max = request.maximumSize
        let scale = min(max.width / image.size.width, max.height / image.size.height)
        let size = CGSize(width: (image.size.width * scale).rounded(), height: (image.size.height * scale).rounded())
        handler(QLThumbnailReply(contextSize: size, currentContextDrawing: {
            image.draw(in: CGRect(origin: .zero, size: size))
            return true
        }), nil)
    }
}
