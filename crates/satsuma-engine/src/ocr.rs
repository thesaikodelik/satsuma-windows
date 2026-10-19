//! On-device text recognition with the Vision framework.

use crate::{Error, Result};
use objc2::rc::autoreleasepool;
use objc2::AnyThread;
use objc2_foundation::{NSArray, NSDictionary, NSURL};
use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};
use std::path::Path;

pub use crate::ocr_layout::{paragraphs, Line};

pub fn recognize(path: &Path) -> Result<Vec<Line>> {
    autoreleasepool(|_| {
        let url = NSURL::from_file_path(path).ok_or_else(|| Error::msg("Invalid image path."))?;
        let handler = unsafe { VNImageRequestHandler::initWithURL_options(VNImageRequestHandler::alloc(), &url, &NSDictionary::new()) };
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        let as_request: &VNRequest = &request;
        handler
            .performRequests_error(&NSArray::from_slice(&[as_request]))
            .map_err(|e| Error::msg(format!("Text recognition failed: {}", e.localizedDescription())))?;
        let mut lines = Vec::new();
        if let Some(results) = request.results() {
            for obs in results.iter() {
                let candidates = obs.topCandidates(1);
                let Some(best) = candidates.iter().next() else { continue };
                let b = unsafe { obs.boundingBox() };
                lines.push(Line {
                    text: best.string().to_string(),
                    x: b.origin.x,
                    top: 1.0 - (b.origin.y + b.size.height),
                    height: b.size.height,
                });
            }
        }
        Ok(lines)
    })
}
