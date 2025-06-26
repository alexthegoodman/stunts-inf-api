use common_layout_2d::{inference::CommonLayoutInference, interface::load_common_layout_2d};
use common_motion_2d_reg::{
    inference::CommonMotionInference, interface::load_common_motion_2d, Wgpu,
};
use std::error::Error;
use std::io::Read;
use tiny_http::{Response, Server};

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

    fn run(&self) -> Result<(), Box<dyn Error>> {
        let server = Server::http("0.0.0.0:8000").expect("Couldn't start server");
        println!("Server running on http://0.0.0.0:8000");

        for mut request in server.incoming_requests() {
            // Handle CORS preflight
            if request.method() == &Method::Options {
                // add https://madebycommon.com for release
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
                            "Access-Control-Allow-Headers: Content-Type, X-Inference-Type",
                        )
                        .unwrap(),
                    );
                request.respond(response)?;
                continue;
            }

            match request.method() {
                Method::Post => {
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

                    // Add CORS headers to the actual response
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