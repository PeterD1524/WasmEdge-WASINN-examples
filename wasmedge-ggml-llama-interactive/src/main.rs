use serde_json::{json, Value};
use std::env;
use std::io::{self, Write};
use wasi_nn::{self, GraphExecutionContext};

fn read_input() -> String {
    loop {
        let mut answer = String::new();
        io::stdin()
            .read_line(&mut answer)
            .expect("Failed to read line");
        if !answer.is_empty() && answer != "\n" && answer != "\r\n" {
            return answer;
        }
    }
}

fn get_options_from_env() -> Value {
    let mut options = json!({});
    match env::var("enable_log") {
        Ok(val) => options["enable-log"] = serde_json::from_str(val.as_str()).unwrap(),
        _ => (),
    };
    match env::var("enable_debug_log") {
        Ok(val) => options["enable-debug-log"] = serde_json::from_str(val.as_str()).unwrap(),
        _ => (),
    };
    match env::var("stream_stdout") {
        Ok(val) => options["stream-stdout"] = serde_json::from_str(val.as_str()).unwrap(),
        _ => (),
    };
    match env::var("n_predict") {
        Ok(val) => options["n-preidct"] = serde_json::from_str(val.as_str()).unwrap(),
        _ => (),
    }
    match env::var("reverse_prompt") {
        Ok(val) => options["reverse-prompt"] = json!(val),
        _ => (),
    }
    match env::var("n_gpu_layers") {
        Ok(val) => options["n-gpu-layers"] = serde_json::from_str(val.as_str()).unwrap(),
        _ => (),
    }
    match env::var("ctx_size") {
        Ok(val) => options["ctx-size"] = serde_json::from_str(val.as_str()).unwrap(),
        _ => (),
    }
    match env::var("batch_size") {
        Ok(val) => options["batch-size"] = serde_json::from_str(val.as_str()).unwrap(),
        _ => (),
    }
    match env::var("temp") {
        Ok(val) => options["temp"] = serde_json::from_str(val.as_str()).unwrap(),
        _ => (),
    }
    match env::var("repeat_penalty") {
        Ok(val) => options["repeat-penalty"] = serde_json::from_str(val.as_str()).unwrap(),
        _ => (),
    }
    match env::var("threads") {
        Ok(val) => options["threads"] = serde_json::from_str(val.as_str()).unwrap(),
        _ => (),
    }

    return options;
}

fn set_data_to_context(
    context: &mut GraphExecutionContext,
    data: Vec<u8>,
) -> Result<(), wasi_nn::Error> {
    context.set_input(0, wasi_nn::TensorType::U8, &[1], &data)
}

#[allow(dead_code)]
fn set_metadata_to_context(
    context: &mut GraphExecutionContext,
    data: Vec<u8>,
) -> Result<(), wasi_nn::Error> {
    context.set_input(1, wasi_nn::TensorType::U8, &[1], &data)
}

fn get_data_from_context(
    context: &GraphExecutionContext,
    index: usize,
    is_compute_single: bool,
) -> String {
    // Preserve for 4096 tokens with average token length 6
    const MAX_OUTPUT_BUFFER_SIZE: usize = 4096 * 6;
    let mut output_buffer = vec![0u8; MAX_OUTPUT_BUFFER_SIZE];
    let mut output_size = if is_compute_single {
        context
            .get_output_single(index, &mut output_buffer)
            .unwrap()
    } else {
        context.get_output(index, &mut output_buffer).unwrap()
    };
    output_size = std::cmp::min(MAX_OUTPUT_BUFFER_SIZE, output_size);

    return String::from_utf8_lossy(&output_buffer[..output_size]).to_string();
}

fn get_output_from_context(context: &GraphExecutionContext, is_compute_single: bool) -> String {
    return get_data_from_context(context, 0, is_compute_single);
}

fn get_metadata_from_context(context: &GraphExecutionContext) -> Value {
    return serde_json::from_str(&get_data_from_context(context, 1, false)).unwrap();
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let model_name: &str = &args[1];
    let options = get_options_from_env();

    // We suuport both compute() and compute_single().
    // compute() will compute the entire paragraph until the end of sequence, and return the entire output.
    // compute_single() will compute one token at a time, and return the output of the last token.
    let mut is_compute_single = false;
    match env::var("compute_single") {
        Ok(val) => is_compute_single = serde_json::from_str(val.as_str()).unwrap(),
        _ => (),
    }

    let graph =
        wasi_nn::GraphBuilder::new(wasi_nn::GraphEncoding::Ggml, wasi_nn::ExecutionTarget::AUTO)
            .config(options.to_string())
            .build_from_cache(model_name)
            .unwrap();
    let mut context = graph.init_execution_context().unwrap();

    // We also support setting the options via input tensor with index 1.
    // Uncomment the line below to run the example, Check our README for more details.
    //
    // set_metadata_to_context(&mut context, options.to_string().as_bytes().to_vec()).unwrap();

    // If there is a third argument, use it as the prompt and enter non-interactive mode.
    // Otherwise, enter interactive mode.
    if args.len() >= 3 {
        let prompt = &args[2];
        println!("Prompt:\n{}", prompt);
        let tensor_data = prompt.as_bytes().to_vec();
        context
            .set_input(0, wasi_nn::TensorType::U8, &[1], &tensor_data)
            .unwrap();
        println!("Response:");
        context.compute().unwrap();
        let output = get_output_from_context(&context, false);
        println!("{}", output.trim());
        std::process::exit(0);
    }

    let system_prompt = String::from("<<SYS>>You are a helpful, respectful and honest assistant. Always answer as short as possible, while being safe. <</SYS>>");
    let mut saved_prompt = String::new();

    loop {
        println!("Question:");
        let input = read_input();
        if saved_prompt == "" {
            saved_prompt = format!("[INST] {} {} [/INST]", system_prompt, input.trim());
        } else {
            saved_prompt = format!("{} [INST] {} [/INST]", saved_prompt, input.trim());
        }

        // Set prompt to the input tensor.
        set_data_to_context(&mut context, saved_prompt.as_bytes().to_vec()).unwrap();

        // Get the number of input tokens.
        let input_metadata = get_metadata_from_context(&context);
        if let Some(true) = options["enable-log"].as_bool() {
            println!("Number of input tokens: {}", input_metadata["input_tokens"]);
        }

        println!("Answer:");

        let mut output = String::new();
        if is_compute_single {
            // Compute one token at a time, and get the token using the get_output_single().
            loop {
                match context.compute_single() {
                    Ok(_) => (),
                    Err(wasi_nn::Error::BackendError(wasi_nn::BackendError::EndOfSequence)) => {
                        break;
                    }
                    Err(wasi_nn::Error::BackendError(wasi_nn::BackendError::ContextFull)) => {
                        println!("[INFO] Context full");
                        break;
                    }
                    Err(wasi_nn::Error::BackendError(wasi_nn::BackendError::PromptTooLong)) => {
                        println!("[INFO] Prompt too long");
                        break;
                    }
                    Err(err) => {
                        println!("[ERROR] {}", err);
                        break;
                    }
                }
                // Retrieve the output.
                let token = get_output_from_context(&context, is_compute_single);
                print!("{}", token);
                io::stdout().flush().unwrap();
                output += &token;
            }
            println!("");
        } else {
            // Blocking: execute the inference.
            context.compute().unwrap();

            // Retrieve the output.
            output = get_output_from_context(&context, is_compute_single);

            // Print the output if not streaming.
            if !options["stream-stdout"].as_bool().unwrap() {
                print!("{}", output.trim());
            }
            println!("");
        }

        saved_prompt = format!("{} {} ", saved_prompt, output.trim());

        // Retrieve the output metadata.
        let metadata = get_metadata_from_context(&context);
        if let Some(true) = options["enable-log"].as_bool() {
            println!("Number of input tokens: {}", metadata["input_tokens"]);
            println!("Number of output tokens: {}", metadata["output_tokens"]);
        }

        // Delete context.
        if is_compute_single {
            context.fini_single().unwrap();
        }
    }
}
