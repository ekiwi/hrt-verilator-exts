use clap::Parser;
use formal::convert::{NamedFsm, Properties};
use parser_verilator::{
    ast::{Design, Domain},
    document::AstDocument,
};
use patronus::expr::{Context, SerializableIrNode, TypeCheck, WidthInt};
use patronus::system::TransitionSystem;
use std::io::BufWriter;
use std::{fs, path::PathBuf, process::ExitCode};

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

    let mut ctx = Context::default();

    let mut model = match NamedFsm::from_design(&mut ctx, &design, args.clock, args.reset) {
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
        model.sys.inputs.len(),
        model
            .sys
            .inputs
            .iter()
            .map(|&i| ctx[i].get_bv_type(&ctx).unwrap())
            .sum::<WidthInt>(),
    );
    println!(
        "registers: {} signals, {} bits",
        model.sys.states.len(),
        model
            .sys
            .states
            .iter()
            .map(|&s| ctx[s.symbol].get_bv_type(&ctx).unwrap())
            .sum::<WidthInt>(),
    );
    println!(
        "outputs: {} signals, {} bits",
        model.sys.outputs.len(),
        model
            .sys
            .outputs
            .iter()
            .map(|&o| ctx[o.expr].get_bv_type(&ctx).unwrap())
            .sum::<WidthInt>(),
    );
    println!(
        "properties: {} assertions, {} assumptions, {} covers",
        model.sys.bad_states.len(),
        model.sys.constraints.len(),
        model.properties.covers.len()
    );

    if let Err(error) = select_property(
        &mut ctx,
        &mut model.sys,
        &mut model.properties,
        args.assert,
        args.cover,
    ) {
        eprintln!("error: {error}");
        return ExitCode::FAILURE;
    }
    if args.debug {
        println!("{}", model.sys.serialize_to_str(&ctx));
    }
    if args.zero_init {
        model.initialize_registers_to_zero();
    }

    if args.strip_symbols {
        todo!("not supported");
    }

    let extension = output
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let contents = match extension.as_deref() {
        Some("btor") | Some("btor2") => {
            let mut out = BufWriter::new(fs::File::create(&output).unwrap());
            patronus::btor2::serialize(&ctx, &mut out, &model.sys).unwrap();
        }
        Some("aag") => todo!("bring back aiger support"),
        Some("aig") => todo!("bring back aiger support"),
        _ => {
            eprintln!(
                "error: output {} must have a .aag or .aig extension",
                output.display()
            );
            return ExitCode::FAILURE;
        }
    };
    println!("wrote: {}", output.display());

    ExitCode::SUCCESS
}

fn select_property(
    ctx: &mut Context,
    sys: &mut TransitionSystem,
    props: &mut Properties,
    assertion_index: Option<usize>,
    cover_index: Option<usize>,
) -> Result<(), String> {
    if let Some(index) = assertion_index {
        let count = sys.bad_states.len();
        let assertion =
            sys.bad_states.get(index).cloned().ok_or_else(|| {
                format!("assertion index {index} is out of range (found {count})")
            })?;
        sys.bad_states.clear();
        sys.bad_states.push(assertion);
        let name = props.asserts[index].clone();
        props.asserts.clear();
        props.asserts.push(name);
    } else if let Some(index) = cover_index {
        let count = props.cover_exprs.len();
        let expr = props
            .cover_exprs
            .get(index)
            .cloned()
            .ok_or_else(|| format!("cover index {index} is out of range (found {count})"))?;
        sys.bad_states.clear();
        sys.bad_states.push(ctx.not(expr));
        props.asserts.clear();
        props.asserts.push(props.covers[index].clone());
    }
    props.covers.clear();
    props.cover_exprs.clear();
    Ok(())
}
