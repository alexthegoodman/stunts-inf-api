use common_motion_2d_reg::{
    inference::CommonMotionInference, interface::load_common_motion_2d, Wgpu,
};
use std::error::Error;
use tiny_http::{Response, Server};

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
            match request.method() {
                tiny_http::Method::Post => {
                    let mut content = String::new();
                    request.as_reader().read_to_string(&mut content)?;

                    let predictions = self.inference.infer(content);
                    let response = serde_json::to_string(&predictions)?;

                    request.respond(Response::from_string(response))?;
                }
                _ => {
                    request.respond(Response::from_string("Method not allowed"))?;
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
