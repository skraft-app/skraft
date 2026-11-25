// The Quick Look preview of a .skraft file (Space in the Finder): the picture it carries, as
// it is (FilePicture.swift). A file without one has no preview.

import Foundation
import QuickLookUI
import UniformTypeIdentifiers

final class PreviewProvider: QLPreviewProvider, QLPreviewingController {
    func providePreview(for request: QLFilePreviewRequest) async throws -> QLPreviewReply {
        guard let png = FilePicture.png(at: request.fileURL), let image = NSImage(data: png) else {
            throw CocoaError(.fileReadCorruptFile)
        }
        return QLPreviewReply(dataOfContentType: .png, contentSize: image.size) { _ in png }
    }
}
