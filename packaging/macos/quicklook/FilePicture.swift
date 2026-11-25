// The picture a .skraft file carries (docs/io/file-format.md): its `thumbnail.png` entry,
// stored uncompressed in the ZIP, found through the central directory and read alone. Shared by
// the Quick Look extensions (scripts/build-quicklook.sh).

import Foundation

enum FilePicture {
    static let entry = "thumbnail.png"
    /// The app gives up on a picture past this size: the extensions do the same.
    static let largest = 4 << 20

    /// The PNG bytes of `url`'s picture; nil for a file without one, or not a ZIP as the app
    /// writes it (one disk, no ZIP64, the entry stored).
    static func png(at url: URL) -> Data? {
        guard let file = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? file.close() }
        guard let length = try? file.seekToEnd(), length >= 22 else { return nil }

        // The end of central directory record: its signature within the last 64 KiB + 22 bytes.
        let tailLength = min(length, 65_557)
        guard let tail = read(file, at: length - tailLength, count: Int(tailLength)) else { return nil }
        guard let end = lastIndex(of: 0x0605_4b50, in: tail) else { return nil }
        let entries = Int(u16(tail, end + 10))
        let directorySize = Int(u32(tail, end + 12))
        let directoryOffset = UInt64(u32(tail, end + 16))
        guard directoryOffset + UInt64(directorySize) <= length,
              let directory = read(file, at: directoryOffset, count: directorySize) else { return nil }

        var at = 0
        for _ in 0..<entries {
            guard at + 46 <= directory.count, u32(directory, at) == 0x0201_4b50 else { return nil }
            let method = u16(directory, at + 10)
            let size = Int(u32(directory, at + 20))
            let nameLength = Int(u16(directory, at + 28))
            let extraLength = Int(u16(directory, at + 30))
            let commentLength = Int(u16(directory, at + 32))
            let localOffset = UInt64(u32(directory, at + 42))
            guard at + 46 + nameLength <= directory.count else { return nil }
            let name = String(decoding: directory[(at + 46)..<(at + 46 + nameLength)], as: UTF8.self)
            if name == entry {
                guard method == 0, size > 0, size <= largest else { return nil }
                // The local header has its own name and extra lengths.
                guard let local = read(file, at: localOffset, count: 30), u32(local, 0) == 0x0403_4b50 else { return nil }
                let start = localOffset + 30 + UInt64(u16(local, 26)) + UInt64(u16(local, 28))
                guard start + UInt64(size) <= length else { return nil }
                return read(file, at: start, count: size)
            }
            at += 46 + nameLength + extraLength + commentLength
        }
        return nil
    }

    private static func read(_ file: FileHandle, at offset: UInt64, count: Int) -> Data? {
        guard (try? file.seek(toOffset: offset)) != nil, let data = try? file.read(upToCount: count),
              data.count == count else { return nil }
        return data
    }

    private static func lastIndex(of signature: UInt32, in data: Data) -> Int? {
        var i = data.count - 22
        while i >= 0 {
            if u32(data, i) == signature { return i }
            i -= 1
        }
        return nil
    }

    private static func u16(_ d: Data, _ i: Int) -> UInt16 {
        let b = d.startIndex + i
        return UInt16(d[b]) | UInt16(d[b + 1]) << 8
    }

    private static func u32(_ d: Data, _ i: Int) -> UInt32 {
        UInt32(u16(d, i)) | UInt32(u16(d, i + 2)) << 16
    }
}
