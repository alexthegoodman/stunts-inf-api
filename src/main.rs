use common_motion_2d_reg::{
    inference::CommonMotionInference, interface::load_common_motion_2d, Wgpu,
};
use std::error::Error;
use tiny_http::{Response, Server};

use std::str::FromStr;
use tiny_http::{Header, Method};

struct ModelServer {
    inference: CommonMotionInference<Wgpu>,
}

impl ModelServer {
    fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            inference: load_common_motion_2d(),
        })
    }

    fn run(&self) -> Result<(), Box<dyn Error>> {
        let server = Server::http("127.0.0.1:8000").expect("Couldn't start server");
        println!("Server running on http://127.0.0.1:8000");

        for mut request in server.incoming_requests() {
            // Handle CORS preflight
            if request.method() == &Method::Options {
                // add https://madebycommon.com for release
                let response = Response::empty(204)
                    .with_header(
                        Header::from_str("Access-Control-Allow-Origin: http://localhost:3000")
                            .unwrap(),
                    )
                    .with_header(
                        Header::from_str("Access-Control-Allow-Methods: POST, OPTIONS").unwrap(),
                    )
                    .with_header(
                        Header::from_str("Access-Control-Allow-Headers: Content-Type").unwrap(),
                    );
                request.respond(response)?;
                continue;
            }

            match request.method() {
                Method::Post => {
                    let mut content = String::new();
                    request.as_reader().read_to_string(&mut content)?;

                    let predictions = self.inference.infer(content);
                    let response_content = serde_json::to_string(&predictions)?;

                    // Add CORS headers to the actual response
                    let response = Response::from_string(response_content)
                        .with_header(
                            Header::from_str("Access-Control-Allow-Origin: http://localhost:3000")
                                .unwrap(),
                        )
                        .with_header(Header::from_str("Content-Type: application/json").unwrap());

                    request.respond(response)?;
                }
                _ => {
                    let response = Response::from_string("Method not allowed")
                        .with_status_code(405)
                        .with_header(
                            Header::from_str("Access-Control-Allow-Origin: http://localhost:3000")
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
