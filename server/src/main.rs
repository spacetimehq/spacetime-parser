use std::{collections::HashMap, sync::Arc};

use actix_web::{middleware, web, App, HttpResponse, HttpServer, Responder};
use base64::Engine;
use error::prelude::*;
use polylang_prover::{compile_program, Inputs, ProgramExt};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProveRequest {
    miden_code: String,
    abi: abi::Abi,
    ctx_public_key: Option<abi::publickey::Key>,
    this: Option<serde_json::Value>,
    this_salts: Option<Vec<u32>>,
    args: Vec<serde_json::Value>,
    other_records: Option<HashMap<String, Vec<(serde_json::Value, Vec<u32>)>>>,
}

async fn prove(
    mut req: web::Json<ProveRequest>,
) -> Result<impl Responder, Box<dyn std::error::Error>> {
    let program = compile_program(&req.abi, &req.miden_code)?;

    let has_this = req.abi.this_type.is_some();
    let this = req.this.clone().unwrap_or(if has_this {
        req.abi.default_this_value()?.try_into()?
    } else {
        serde_json::Value::Null
    });

    if !has_this {
        req.abi.this_type = Some(abi::Type::Struct(abi::Struct {
            name: "Empty".to_string(),
            fields: Vec::new(),
        }));
        req.abi.this_addr = Some(0);
    }

    let inputs = Inputs::new(
        req.abi.clone(),
        req.ctx_public_key.clone(),
        req.this_salts.clone().unwrap_or_default(),
        this.clone(),
        req.args.clone(),
        req.other_records.clone().unwrap_or_default(),
    )?;

    let program_info = program.clone().to_program_info_bytes();
    let output = tokio::task::spawn_blocking({
        let inputs = inputs.clone();
        move || polylang_prover::prove(&program, &inputs).map_err(|e| e.to_string())
    })
    .await??;
    let new_this = TryInto::<serde_json::Value>::try_into(output.new_this)?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "old": {
            "this": this,
            "hashes": inputs.this_field_hashes,
        },
        "new": {
            "selfDestructed": output.self_destructed,
            "this": new_this,
            "hashes": output.new_hashes,
        },
        "stack": {
            "input": output.input_stack,
            "output": output.stack,
        },
        "programInfo": base64::engine::general_purpose::STANDARD.encode(program_info),
        "proof": base64::engine::general_purpose::STANDARD.encode(output.proof),
        "debug": {
            "logs": output.run_output.logs(),
        }
    })))
}

#[tokio::main]
async fn main() {
    let app = || {
        App::new()
            .wrap(middleware::Compress::default())
            .service(web::resource("/prove").route(web::post().to(prove)))
    };

    HttpServer::new(move || app())
        .bind(("127.0.0.1", 3000))
        .unwrap()
        .run()
        .await
        .unwrap();
}