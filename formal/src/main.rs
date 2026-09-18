use std::{fs, path::PathBuf, process::ExitCode};

use clap::Parser;
use formal_utils::{
    formats::aiger::{AigerVersion, ascii::write_aiger_ascii, binary::write_aiger_binary},
    fsm::{FSM, verify::VerifyOrdering},
};

use formal::convert::NamedFsm;
use parser_verilator::{
    ast::{Design, Domain},
    document::AstDocument,
};

#[derive(Debug, Parser)]
#[command(about = "Convert a Verilator JSON AST to an ordered Boolean FSM")]
struct Args {
    /// Path to a JSON AST emitted by Verilator.
    tree_json: PathBuf,

    /// Clock signal and edge; prefix the name with ! for a negative edge.
    #[arg(long)]
    clock: Domain,

    /// Optional reset signal; prefix the name with ! for active-low polarity.
    #[arg(long)]
    reset: Option<Domain>,

    /// Write btor.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Write the parsed, typed design as pretty-printed RON.
    #[arg(long)]
    ron_output: Option<PathBuf>,

    /// Omit input, latch, output, and property names from the AIGER output.
    #[arg(long)]
    strip_symbols: bool,

    /// Initialize every register to zero in the AIGER output.
    #[arg(long)]
    zero_init: bool,

    /// Export only the assertion at this zero-based index.
    #[arg(long, requires = "output")]
    assert: Option<usize>,

    /// Export only the cover at this zero-based index, inverted as an assertion.
    #[arg(long, conflicts_with = "assert", requires = "output")]
    cover: Option<usize>,

    /// Print every AIGER symbol that would normally be written.
    #[arg(long)]
    debug: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();

    let document = match AstDocument::from_path(&args.tree_json) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let design = match Design::try_from(&document) {
        Ok(design) => design,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(output) = args.ron_output {
        let contents = match ron::ser::to_string_pretty(&design, ron::ser::PrettyConfig::default())
        {
            Ok(contents) => contents,
            Err(error) => {
                eprintln!("error: could not serialize design as RON: {error}");
                return ExitCode::FAILURE;
            }
        };
        if let Err(error) = fs::write(&output, contents) {
            eprintln!("error: could not write {}: {error}", output.display());
            return ExitCode::FAILURE;
        }
        println!("wrote RON: {}", output.display());
    }

    let Some(output) = args.output else {
        return ExitCode::SUCCESS;
    };

    let mut model = match NamedFsm::from_design(&design, args.clock, args.reset) {
        Ok(model) => model,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!("converted: {}", args.tree_json.display());
    println!(
        "clock: {:?} edge of {}",
        model.clock.domain.edge, model.clock.domain.name
    );
    println!(
        "inputs: {} signals, {} bits",
        model.inputs.len(),
        model.fsm.get_inputs().len()
    );
    println!(
        "registers: {} signals, {} bits",
        model.registers.len(),
        model.fsm.get_latches().len()
    );
    println!(
        "outputs: {} signals, {} bits",
        model.outputs.len(),
        model.fsm.get_outputs().len()
    );
    println!("gates: {}", model.fsm.get_gates().len());
    println!(
        "properties: {} assertions, {} assumptions, {} covers",
        model.assertions.len(),
        model.assumptions.len(),
        model.covers.len()
    );

    if let Err(error) = select_property(&mut model.fsm, args.assert, args.cover) {
        eprintln!("error: {error}");
        return ExitCode::FAILURE;
    }
    if args.debug {
        print_aiger_symbols(&model.fsm);
    }
    if args.zero_init {
        model.initialize_registers_to_zero();
    }
    model.fsm.normalize_outputs();
    model.fsm.reorder_gates();
    model.fsm.verify(VerifyOrdering::Verify);
    if args.strip_symbols {
        clear_symbols(&mut model.fsm);
    }

    let extension = output
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let contents = match extension.as_deref() {
        Some("aag") => write_aiger_ascii(&model.fsm, AigerVersion::V1_9),
        Some("aig") => write_aiger_binary(&model.fsm, AigerVersion::V1_9),
        _ => {
            eprintln!(
                "error: output {} must have a .aag or .aig extension",
                output.display()
            );
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = fs::write(&output, contents) {
        eprintln!("error: could not write {}: {error}", output.display());
        return ExitCode::FAILURE;
    }
    println!("wrote: {}", output.display());

    ExitCode::SUCCESS
}

fn select_property(
    fsm: &mut FSM,
    assertion_index: Option<usize>,
    cover_index: Option<usize>,
) -> Result<(), String> {
    if let Some(index) = assertion_index {
        let count = fsm.get_asserts().len();
        let assertion =
            fsm.get_asserts().get(index).cloned().ok_or_else(|| {
                format!("assertion index {index} is out of range (found {count})")
            })?;
        fsm.get_asserts_mut().clear();
        fsm.get_asserts_mut().push(assertion);
    } else if let Some(index) = cover_index {
        let count = fsm.get_covers().len();
        let (value, label) = fsm
            .get_covers()
            .get(index)
            .cloned()
            .ok_or_else(|| format!("cover index {index} is out of range (found {count})"))?;
        fsm.get_asserts_mut().clear();
        fsm.get_asserts_mut().push((!value, label));
    }
    fsm.get_covers_mut().clear();
    Ok(())
}

fn print_aiger_symbols(fsm: &FSM) {
    for (index, input) in fsm.get_inputs().into_iter().enumerate() {
        if let Some(label) = fsm.get_variable_label(input.index()) {
            eprintln!("aiger symbol: i{index} {label}");
        }
    }
    for (index, latch) in fsm.get_latches().into_iter().enumerate() {
        if let Some(label) = fsm.get_variable_label(latch.output.index()) {
            eprintln!("aiger symbol: l{index} {label}");
        }
    }
    for (index, (_, label)) in fsm.get_outputs().into_iter().enumerate() {
        if let Some(label) = label {
            eprintln!("aiger symbol: o{index} {label}");
        }
    }
    for (index, (_, label)) in fsm.get_asserts().into_iter().enumerate() {
        if let Some(label) = label {
            eprintln!("aiger symbol: b{index} {label}");
        }
    }
    for (index, (_, label)) in fsm.get_assumes().into_iter().enumerate() {
        if let Some(label) = label {
            eprintln!("aiger symbol: c{index} {label}");
        }
    }
}

fn clear_symbols(fsm: &mut FSM) {
    for variable_index in 0..fsm.get_num_variables() {
        *fsm.get_variable_label_mut(variable_index) = None;
    }
    for (_, label) in fsm.get_outputs_mut() {
        *label = None;
    }
    for (_, label) in fsm.get_asserts_mut() {
        *label = None;
    }
    for (_, label) in fsm.get_assumes_mut() {
        *label = None;
    }
    for (_, label) in fsm.get_covers_mut() {
        *label = None;
    }
}
