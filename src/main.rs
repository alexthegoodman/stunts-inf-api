use common_layout_2d::{inference::CommonLayoutInference, interface::load_common_layout_2d};
use common_motion_2d_reg::{
    inference::CommonMotionInference, interface::load_common_motion_2d, Wgpu,
};
use std::error::Error;
use std::io::{Read, Write};
use std::process::Command;
use std::fs;
use std::path::Path;
use tiny_http::{Response, Server};
use uuid::Uuid;

use std::str::FromStr;
use tiny_http::{Header, Method};

struct ModelServer {
    motion_inference: CommonMotionInference<Wgpu>,
    layout_inference: CommonLayoutInference<Wgpu>,
}

impl ModelServer {
    fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            motion_inference: load_common_motion_2d(),
            layout_inference: load_common_layout_2d(),
        })
    }

    fn resize_video(&self, video_data: &[u8], max_width: u32, max_height: u32) -> Result<Vec<u8>, Box<dyn Error>> {
        // Create temporary files
        let temp_id = Uuid::new_v4().to_string();
        let input_path = format!("/tmp/input_{}.mp4", temp_id);
        let output_path = format!("/tmp/output_{}.mp4", temp_id);

        // Write input video to temporary file
        fs::write(&input_path, video_data)?;

        // Get video dimensions first
        let probe_output = Command::new("ffprobe")
            .args([
                "-v", "quiet",
                "-print_format", "json",
                "-show_streams",
                &input_path
            ])
            .output()?;

        if !probe_output.status.success() {
            return Err("Failed to probe video".into());
        }

        let probe_json: serde_json::Value = serde_json::from_slice(&probe_output.stdout)?;
        let video_stream = probe_json["streams"]
            .as_array()
            .ok_or("No streams found")?
            .iter()
            .find(|s| s["codec_type"] == "video")
            .ok_or("No video stream found")?;

        // let original_width = video_stream["width"].as_u64().unwrap_or(0) as u32;
        // let original_height = video_stream["height"].as_u64().unwrap_or(0) as u32;

        // // Calculate new dimensions maintaining aspect ratio
        // let (new_width, new_height) = if original_width > max_width || original_height > max_height {
        //     let width_ratio = max_width as f64 / original_width as f64;
        //     let height_ratio = max_height as f64 / original_height as f64;
        //     let scale_ratio = width_ratio.min(height_ratio);
            
        //     let scaled_width = (original_width as f64 * scale_ratio) as u32;
        //     let scaled_height = (original_height as f64 * scale_ratio) as u32;
            
        //     // Ensure dimensions are even (required for some codecs)
        //     (scaled_width & !1, scaled_height & !1)
        // } else {
        //     (original_width, original_height)
        // };

        // println!("Resizing video from {}x{} to {}x{}", original_width, original_height, new_width, new_height);

        let original_width = video_stream["width"].as_u64().unwrap_or(0) as u32;
        let original_height = video_stream["height"].as_u64().unwrap_or(0) as u32;

        // Calculate new dimensions maintaining aspect ratio and ensuring no warping
        let (new_width, new_height) = if original_width > max_width || original_height > max_height {
            let width_ratio = max_width as f64 / original_width as f64;
            let height_ratio = max_height as f64 / original_height as f64;
            let scale_ratio = width_ratio.min(height_ratio);
            
            let scaled_width = (original_width as f64 * scale_ratio) as u32;
            let scaled_height = (original_height as f64 * scale_ratio) as u32;
            
            // Ensure dimensions are even (required for H.264) and maintain exact aspect ratio
            let even_width = scaled_width & !1;
            let even_height = scaled_height & !1;
            
            // Double-check aspect ratio is maintained
            let original_aspect = original_width as f64 / original_height as f64;
            let new_aspect = even_width as f64 / even_height as f64;
            
            // If making both dimensions even changed the aspect ratio significantly, 
            // adjust one dimension to maintain the ratio
            if (original_aspect - new_aspect).abs() > 0.01 {
                if even_width as f64 / original_aspect < even_height as f64 {
                    // Adjust height to match width
                    let correct_height = (even_width as f64 / original_aspect) as u32 & !1;
                    (even_width, correct_height)
                } else {
                    // Adjust width to match height
                    let correct_width = (even_height as f64 * original_aspect) as u32 & !1;
                    (correct_width, even_height)
                }
            } else {
                (even_width, even_height)
            }
        } else {
            (original_width, original_height)
        };

        println!("Resizing video from {}x{} to {}x{} (aspect ratio: {:.3} -> {:.3})", 
            original_width, original_height, new_width, new_height,
            original_width as f64 / original_height as f64,
            new_width as f64 / new_height as f64);
        
        // Run FFmpeg to resize and compress
        let ffmpeg_output = Command::new("ffmpeg")
            .args([
                "-i", &input_path,
                "-vf", &format!("scale={}:{}", new_width, new_height),
                "-c:v", "libx264",
                // intended to match WebCodecs profile / level
                //"-profile:v", "main", // 4D = Main Profile
                //"-level", "5.0", // 32 = Level 5.0
                //"-preset", "medium",
                // trying more compatible settings
                // "-level", "3.1", // Much more conservative level
                // most compatible settings for web?
                "-profile:v", "baseline", // 42 = Baseline Profile (matches working videos)
                "-level", "4.0", // 28 = Level 4.0 (matches working videos)
                "-preset", "fast", // Faster, simpler encoding
                "-crf", "23", // Compression quality (18-28, lower = better quality)
                "-c:a", "aac",
                "-b:a", "128k",
                "-movflags", "+faststart", // Optimize for web streaming
                "-y", // Overwrite output file
                &output_path
            ])
            .output()?;

        // Clean up input file
        let _ = fs::remove_file(&input_path);

        if !ffmpeg_output.status.success() {
            let _ = fs::remove_file(&output_path);
            let error_msg = String::from_utf8_lossy(&ffmpeg_output.stderr);
            return Err(format!("FFmpeg failed: {}", error_msg).into());
        }

        // Read the processed video
        let processed_video = fs::read(&output_path)?;
        
        // Clean up output file
        let _ = fs::remove_file(&output_path);

        Ok(processed_video)
    }

    fn run(&self) -> Result<(), Box<dyn Error>> {
        let server = Server::http("0.0.0.0:8000").expect("Couldn't start server");
        println!("Server running on http://0.0.0.0:8000");

        for mut request in server.incoming_requests() {
            let url = request.url();
            
            // Handle CORS preflight
            if request.method() == &Method::Options {
                let response = Response::empty(204)
                    .with_header(
                        Header::from_str("Access-Control-Allow-Origin: https://madebycommon.com")
                            .unwrap(),
                    )
                    .with_header(
                        Header::from_str("Access-Control-Allow-Methods: POST, OPTIONS").unwrap(),
                    )
                    .with_header(
                        Header::from_str(
                            "Access-Control-Allow-Headers: Content-Type, X-Inference-Type, X-Max-Width, X-Max-Height",
                        )
                        .unwrap(),
                    );
                request.respond(response)?;
                continue;
            }

            match (request.method(), url) {
                (Method::Post, "/inference") => {
                    let mut content = String::new();
                    request.as_reader().read_to_string(&mut content)?;

                    // Check for the inference type header
                    let inference_type = request
                        .headers()
                        .iter()
                        .find(|h| h.field.equiv("X-Inference-Type"))
                        .map(|h| h.value.as_str())
                        .unwrap_or("motion");

                    let response_content = match inference_type {
                        "layout" => {
                            println!("Performing layout inference");
                            let predictions = self.layout_inference.infer(content);
                            serde_json::to_string(&predictions)?
                        }
                        _ => {
                            // Default to motion inference
                            println!("Performing motion inference");
                            let predictions = self.motion_inference.infer(content);
                            serde_json::to_string(&predictions)?
                        }
                    };

                    let response = Response::from_string(response_content)
                        .with_header(
                            Header::from_str("Access-Control-Allow-Origin: https://madebycommon.com")
                                .unwrap(),
                        )
                        .with_header(Header::from_str("Content-Type: application/json").unwrap());

                    request.respond(response)?;
                }
                (Method::Post, "/resize-video") => {
                    // Read video data from request body
                    let mut video_data = Vec::new();
                    request.as_reader().read_to_end(&mut video_data)?;

                    // Get max dimensions from headers (default to 1920x1080)
                    let max_width = request
                        .headers()
                        .iter()
                        .find(|h| h.field.equiv("X-Max-Width"))
                        .and_then(|h| h.value.as_str().parse::<u32>().ok())
                        .unwrap_or(1920);

                    let max_height = request
                        .headers()
                        .iter()
                        .find(|h| h.field.equiv("X-Max-Height"))
                        .and_then(|h| h.value.as_str().parse::<u32>().ok())
                        .unwrap_or(1080);

                    match self.resize_video(&video_data, max_width, max_height) {
                        Ok(processed_video) => {
                            let response = Response::from_data(processed_video)
                                .with_header(
                                    Header::from_str("Access-Control-Allow-Origin: https://madebycommon.com")
                                        .unwrap(),
                                )
                                .with_header(Header::from_str("Content-Type: video/mp4").unwrap());

                            request.respond(response)?;
                        }
                        Err(e) => {
                            println!("Video processing error: {}", e);
                            let error_response = serde_json::json!({
                                "error": "Failed to process video",
                                "details": e.to_string()
                            });
                            
                            let response = Response::from_string(error_response.to_string())
                                .with_status_code(500)
                                .with_header(
                                    Header::from_str("Access-Control-Allow-Origin: https://madebycommon.com")
                                        .unwrap(),
                                )
                                .with_header(Header::from_str("Content-Type: application/json").unwrap());

                            request.respond(response)?;
                        }
                    }
                }
                (Method::Post, _) => {
                    // Handle legacy inference endpoint (without /inference path)
                    let mut content = String::new();
                    request.as_reader().read_to_string(&mut content)?;

                    let inference_type = request
                        .headers()
                        .iter()
                        .find(|h| h.field.equiv("X-Inference-Type"))
                        .map(|h| h.value.as_str())
                        .unwrap_or("motion");

                    let response_content = match inference_type {
                        "layout" => {
                            println!("Performing layout inference");
                            let predictions = self.layout_inference.infer(content);
                            serde_json::to_string(&predictions)?
                        }
                        _ => {
                            println!("Performing motion inference");
                            let predictions = self.motion_inference.infer(content);
                            serde_json::to_string(&predictions)?
                        }
                    };

                    let response = Response::from_string(response_content)
                        .with_header(
                            Header::from_str("Access-Control-Allow-Origin: https://madebycommon.com")
                                .unwrap(),
                        )
                        .with_header(Header::from_str("Content-Type: application/json").unwrap());

                    request.respond(response)?;
                }
                _ => {
                    let response = Response::from_string("Method not allowed")
                        .with_status_code(405)
                        .with_header(
                            Header::from_str("Access-Control-Allow-Origin: https://madebycommon.com")
                                .unwrap(),
                        );
                    request.respond(response)?;
                }
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let server = ModelServer::new()?;
    server.run()
}