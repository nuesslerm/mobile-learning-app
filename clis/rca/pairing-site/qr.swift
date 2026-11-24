import CoreImage
import CoreImage.CIFilterBuiltins
import Foundation

enum QRFailure: Error {
    case encodingFailed
}

let filter = CIFilter.qrCodeGenerator()
filter.message = Data(CommandLine.arguments[1].utf8)
filter.correctionLevel = "M"
guard let code = filter.outputImage else { throw QRFailure.encodingFailed }

let background = CIImage(color: .white).cropped(to: code.extent.insetBy(dx: -4, dy: -4))
let image = code.samplingNearest().composited(over: background)
    .transformed(by: CGAffineTransform(scaleX: 8, y: 8))
guard let png = CIContext().pngRepresentation(
    of: image, format: .RGBA8, colorSpace: CGColorSpaceCreateDeviceRGB()
) else { throw QRFailure.encodingFailed }
FileHandle.standardOutput.write(png)
