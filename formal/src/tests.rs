use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

use parser_verilator::{
    ast::{BlockKind, Design, Domain, Edge, ExpressionKind, StatementKind},
    document::AstDocument,
};

use crate::convert::{NamedFsm, NamedProperty, NamedSignal};

#[test]
fn domains_parse_positive_and_negative_edges() {
    assert_eq!(
        "clk".parse::<Domain>().unwrap(),
        Domain::new("clk", Edge::Positive)
    );
    assert_eq!(
        "!clkn".parse::<Domain>().unwrap(),
        Domain::new("clkn", Edge::Negative)
    );
    assert!("!".parse::<Domain>().is_err());
}

#[test]
fn explicit_clock_must_match_the_ast_clock_name_and_edge() {
    let document = AstDocument::from_path(build_fixture("counter")).unwrap();

    let wrong_name =
        NamedFsm::from_document(&document, Domain::new("other_clk", Edge::Positive), None)
            .err()
            .unwrap();
    assert!(
        wrong_name
            .to_string()
            .contains(r#"expected clock Domain { name: "other_clk", edge: Positive }"#)
    );

    let wrong_edge = NamedFsm::from_document(&document, Domain::new("clk", Edge::Negative), None)
        .err()
        .unwrap();
    assert!(
        wrong_edge
            .to_string()
            .contains(r#"expected clock Domain { name: "clk", edge: Negative }"#)
    );
}

#[test]
fn specified_reset_initializes_registers_and_is_tied_inactive() {
    let document = AstDocument::from_path(build_fixture("counter")).unwrap();
    let design = Design::try_from(&document).unwrap();
    let model = NamedFsm::from_design(
        &design,
        Domain::new("clk", Edge::Positive),
        Some(Domain::new("reset_n", Edge::Negative)),
    )
    .unwrap();

    assert_eq!(signal_names(&model.inputs), vec!["enable"]);
    assert_eq!(model.fsm.get_inputs().len(), 1);
    let count = (&model.registers)
        .into_iter()
        .find(|register| register.name == "count")
        .unwrap();
    for bit in &count.bits {
        let latch = model
            .fsm
            .get_latch(bit.value.unwrap_variable().index())
            .unwrap();
        assert_eq!(latch.reset_value, Some(false));
    }
    assert_eq!(
        model
            .fsm
            .get_variable_label(count.bits[0].value.unwrap_variable().index()),
        &Some("count[0]".to_owned())
    );

    let mut simulator = Simulator::from(model.fsm.clone());
    set_input(&model, &mut simulator, "enable", true);
    simulator.eval();
    simulator.step();
    simulator.eval();
    assert_eq!(output_word(&simulator), 1);
}

#[test]
fn active_low_reset_polarity_is_used_for_initial_values() {
    let document = AstDocument::from_path(build_fixture("counter_ones")).unwrap();
    let design = Design::try_from(&document).unwrap();
    let model = NamedFsm::from_design(
        &design,
        Domain::new("clk", Edge::Positive),
        Some(Domain::new("reset_n", Edge::Negative)),
    )
    .unwrap();
    let count = (&model.registers)
        .into_iter()
        .find(|register| register.name == "count")
        .unwrap();

    for bit in &count.bits {
        let latch = model
            .fsm
            .get_latch(bit.value.unwrap_variable().index())
            .unwrap();
        assert_eq!(latch.reset_value, Some(true));
    }
}

#[test]
fn unused_specified_reset_is_removed_without_initializing_registers() {
    let document = AstDocument::from_path(build_fixture("case_statements")).unwrap();
    let design = Design::try_from(&document).unwrap();
    let model = NamedFsm::from_design(
        &design,
        Domain::new("clk", Edge::Positive),
        Some(Domain::new("reset_n", Edge::Negative)),
    )
    .unwrap();

    assert_eq!(signal_names(&model.inputs), vec!["selector", "data"]);
    let decoded = (&model.registers)
        .into_iter()
        .find(|register| register.name == "decoded")
        .unwrap();
    for bit in &decoded.bits {
        let latch = model
            .fsm
            .get_latch(bit.value.unwrap_variable().index())
            .unwrap();
        assert_eq!(latch.reset_value, None);
    }
}

#[test]
fn specified_reset_allows_a_matching_secondary_sensitivity_edge() {
    let input = r#"
    {
      "type":"NETLIST",
      "nodesp":[
        {"type":"BASICDTYPE","addr":"(D)"},
        {"type":"VAR","addr":"(C)","name":"clk","origName":"clk","dtypep":"(D)","direction":"INPUT","varType":"PORT"},
        {"type":"VAR","addr":"(R)","name":"rst_n","origName":"rst_n","dtypep":"(D)","direction":"INPUT","varType":"PORT"},
        {"type":"VAR","addr":"(Q)","name":"state","origName":"state","dtypep":"(D)","direction":"INPUT","varType":"PORT"},
        {"type":"SENTREE","addr":"(S)","sensesp":[
          {"type":"SENITEM","edgeType":"POS","sensp":{"type":"VARREF","varp":"(C)","dtypep":"(D)","access":"RD"}},
          {"type":"SENITEM","edgeType":"NEG","sensp":{"type":"VARREF","varp":"(R)","dtypep":"(D)","access":"RD"}}
        ]},
        {"type":"SCOPE","addr":"(P)","name":"TOP","blocksp":[
          {"type":"ACTIVE","name":"sequent","sentreep":"(S)","stmtsp":[
            {"type":"ASSIGN","lhsp":{"type":"VARREF","varp":"(Q)","dtypep":"(D)","access":"WR"},"rhsp":{"type":"VARREF","varp":"(Q)","dtypep":"(D)","access":"RD"}}
          ]}
        ]}
      ]
    }
    "#;
    let document = AstDocument::from_reader(input.as_bytes()).unwrap();
    let clock = Domain::new("clk", Edge::Positive);

    let design = Design::try_from(&document).unwrap();
    assert_eq!(design.sensitivity_domains.len(), 2);
    let model = NamedFsm::from_design(
        &design,
        clock.clone(),
        Some(Domain::new("rst_n", Edge::Negative)),
    )
    .unwrap();
    assert_eq!(model.clock.domain.name, "clk");
    assert_eq!(model.clock.domain.edge, Edge::Positive);

    let error = NamedFsm::from_design(&design, clock, None)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("encountered SignalDomain"));
    assert!(error.contains(r#"name: "rst_n", edge: Negative"#));
}

#[test]
fn typed_ast_retains_initial_blocks_but_aiger_conversion_rejects_them() {
    let input = r#"
    {
      "type":"NETLIST",
      "nodesp":[
        {"type":"BASICDTYPE","addr":"(D)"},
        {"type":"VAR","addr":"(C)","name":"clk","origName":"clk","dtypep":"(D)","direction":"INPUT","varType":"PORT"},
        {"type":"VAR","addr":"(Q)","name":"state","origName":"state","dtypep":"(D)","varType":"VAR"},
        {"type":"SENTREE","addr":"(S)","sensesp":[
          {"type":"SENITEM","edgeType":"POS","sensp":{"type":"VARREF","varp":"(C)","dtypep":"(D)","access":"RD"}}
        ]},
        {"type":"SCOPE","addr":"(P)","name":"TOP","blocksp":[
          {"type":"ACTIVE","name":"","stmtsp":[
            {"type":"INITIAL","loc":"test.sv,2:3,2:10","stmtsp":[
              {"type":"ASSIGN","lhsp":{"type":"VARREF","varp":"(Q)","dtypep":"(D)","access":"WR"},"rhsp":{"type":"CONST","name":"1'h0","dtypep":"(D)"}}
            ]}
          ]},
          {"type":"ACTIVE","name":"sequent","sentreep":"(S)","stmtsp":[
            {"type":"ASSIGN","lhsp":{"type":"VARREF","varp":"(Q)","dtypep":"(D)","access":"WR"},"rhsp":{"type":"VARREF","varp":"(Q)","dtypep":"(D)","access":"RD"}}
          ]}
        ]}
      ]
    }
    "#;
    for node_type in ["INITIAL", "INITIALSTATIC"] {
        let input = input.replace("\"INITIAL\"", &format!("\"{node_type}\""));
        let document = AstDocument::from_reader(input.as_bytes()).unwrap();
        let design = Design::try_from(&document).unwrap();

        assert_eq!(design.initial.len(), 1);
        match &design.initial[0].kind {
            StatementKind::Block { kind, .. } => {
                assert_eq!(*kind, BlockKind::Initial);
            }
            kind => panic!("expected initial block, found {kind:?}"),
        }

        let error = NamedFsm::from_design(&design, Domain::new("clk", Edge::Positive), None)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(&format!("{node_type} at test.sv,2:3,2:10")));
        assert!(error.contains("not supported by AIGER conversion"));
    }
}

#[test]
fn nonzero_formal_static_initialization_is_rejected() {
    let document = AstDocument::from_path(build_fixture("counter")).unwrap();
    let mut design = Design::try_from(&document).unwrap();
    assert_eq!(design.initial[0].source.node_type, "INITIALSTATIC");
    let StatementKind::Block { statements, .. } = &mut design.initial[0].kind else {
        panic!("expected static initializer block");
    };
    let StatementKind::Assignment { value, .. } = &mut statements[0].kind else {
        panic!("expected static initializer assignment");
    };
    let ExpressionKind::Constant(literal) = &mut value.kind else {
        panic!("expected constant initializer");
    };
    literal.value = 1u32.into();
    literal.spelling = "4'h1".to_string();
    let error = NamedFsm::from_design(&design, Domain::new("clk", Edge::Positive), None)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("INITIALSTATIC"));
    assert!(error.contains("not supported by AIGER conversion"));
}

#[test]
fn compatibility_conversion_matches_explicit_phases() {
    let document = AstDocument::from_path(build_fixture("counter")).unwrap();
    let design = Design::try_from(&document).unwrap();
    let mut explicit = NamedFsm::try_from(&design).unwrap();
    let mut compatibility = NamedFsm::try_from(&document).unwrap();

    prepare_model_for_export(&mut explicit);
    prepare_model_for_export(&mut compatibility);

    assert_eq!(
        write_aiger_ascii(&explicit.fsm, AigerVersion::V1_9),
        write_aiger_ascii(&compatibility.fsm, AigerVersion::V1_9)
    );
    assert_eq!(
        signal_names(&explicit.inputs),
        signal_names(&compatibility.inputs)
    );
    assert_eq!(
        property_names(&explicit.assertions),
        property_names(&compatibility.assertions)
    );
}

#[test]
fn counter_fixture() {
    let mut model = load_model("counter");

    verify_counter_model(&model);
    verify_formal_history_initialization(&model);
    simulate_counter(&model);
    verify_with_ric3(&model, &[]);
    verify_aag_export(&mut model);
}

#[test]
fn combinational_loops_fixture() {
    let model = load_model("combinational_loops");

    assert_eq!(signal_names(&model.outputs), vec!["total"]);
    assert_eq!(property_names(&model.assertions), vec!["assert_total"]);
    model.fsm.verify(VerifyOrdering::Verify);
    verify_with_ric3(&model, &[]);
}

#[test]
fn counter_free_fixture() {
    let model = load_model("counter_free");

    assert_eq!(model.clock.domain.name, "clk");
    assert_eq!(signal_names(&model.inputs), vec!["reset_n", "enable"]);
    assert_eq!(model.fsm.get_inputs().len(), 2);
    assert_eq!(model.fsm.get_latches().len(), 41);
    assert_eq!(model.assertions.len(), 1);
    assert_eq!(model.assumptions.len(), 1);
    assert_eq!(model.covers.len(), 1);
    verify_with_ric3(&model, &[]);
}

#[test]
fn counter_ones_fixture() {
    let model = load_model("counter_ones");

    assert_eq!(
        property_names(&model.assertions),
        vec!["assert_holds_when_disabled"]
    );
    assert!(model.assumptions.is_empty());
    verify_formal_history_initialization(&model);
    simulate_counter_ones(&model);
    verify_with_ric3(&model, &[]);
}

#[test]
fn case_statements_fixture() {
    let model = load_model("case_statements");

    assert_eq!(model.clock.domain.name, "clk");
    assert_eq!(
        signal_names(&model.inputs),
        vec!["reset_n", "selector", "data"]
    );
    let mut outputs = signal_names(&model.outputs);
    outputs.sort();
    assert_eq!(
        outputs,
        vec!["combinational_decoded", "decoded", "wildcard_decoded"]
    );
    let mut assertions = property_names(&model.assertions);
    assertions.sort();
    assert_eq!(
        assertions,
        vec![
            "assert_wildcard_decode_matches",
            "assert_zero_case_captures_data"
        ]
    );
    model.fsm.verify(VerifyOrdering::Verify);
    simulate_case_statements(&model);
    verify_with_ric3(&model, &[]);
}

#[test]
fn wire_ports_fixture() {
    let model = load_model("wire_ports");

    assert_eq!(model.clock.domain.name, "clk");
    assert_eq!(signal_names(&model.inputs), vec!["reset_n", "data_in"]);
    assert_eq!(signal_names(&model.outputs), vec!["data_out"]);
    assert_eq!(model.fsm.get_inputs().len(), 5);
    assert_eq!(model.fsm.get_latches().len(), 4);
    assert_eq!(model.fsm.get_outputs().len(), 4);
    assert_eq!(
        property_names(&model.assertions),
        vec!["assert_output_matches_register"]
    );
    model.fsm.verify(VerifyOrdering::Verify);
}

#[test]
fn package_properties_fixture() {
    let model = load_model_with_reset("package_properties");

    assert_eq!(model.clock.domain.name, "clk");
    assert_eq!(signal_names(&model.inputs), vec!["enable"]);
    assert_eq!(signal_names(&model.outputs), vec!["count"]);
    assert_eq!(
        property_names(&model.assertions),
        vec!["assert_reset_clears_count", "assert_disabled_holds_count"]
    );
    model.fsm.verify(VerifyOrdering::Verify);
}

#[test]
fn public_submodules_fixture() {
    let model = load_model_with_reset("public_submodules");

    assert_eq!(model.clock.domain.name, "clk");
    assert_eq!(signal_names(&model.inputs), vec!["enable0", "enable1"]);
    assert_eq!(
        signal_names(&model.registers),
        vec![
            "public_submodules.counter0.state",
            "public_submodules.counter1.state"
        ]
    );
    assert_eq!(model.fsm.get_latches().len(), 8);
    assert_eq!(signal_names(&model.outputs), vec!["count0", "count1"]);
    assert_eq!(
        property_names(&model.assertions),
        vec![
            "public_submodules.counter0.assert_output_matches_state",
            "public_submodules.counter1.assert_output_matches_state"
        ]
    );
    model.fsm.verify(VerifyOrdering::Verify);
}

#[test]
fn signed_operations_fixture() {
    let model = load_model("signed_operations");

    assert_eq!(signal_names(&model.inputs), vec!["reset_n", "lhs", "rhs"]);
    let mut outputs = signal_names(&model.outputs);
    outputs.sort();
    assert_eq!(
        outputs,
        vec![
            "captured_signed_product",
            "captured_signed_quotient",
            "captured_sum",
            "captured_unsigned_product",
            "captured_unsigned_quotient",
        ]
    );
    let mut assertions = property_names(&model.assertions);
    assertions.sort();
    assert_eq!(
        assertions,
        vec![
            "assert_signed_add_matches_unsigned",
            "assert_signed_gt_matches_biased_unsigned",
            "assert_signed_gte_matches_biased_unsigned",
            "assert_signed_lt_matches_biased_unsigned",
            "assert_signed_lte_matches_biased_unsigned",
            "assert_signed_mul_matches_unsigned",
            "assert_signed_sub_matches_unsigned",
        ]
    );
    model.fsm.verify(VerifyOrdering::Verify);
    simulate_signed_operations(&model);
    verify_with_ric3(&model, &[]);
}

#[test]
fn multidim_arrays_fixture() {
    let model = load_model("multidim_arrays");

    assert_eq!(model.clock.domain.name, "clk");
    assert_eq!(
        signal_names(&model.inputs),
        vec![
            "reset_n",
            "write_enable",
            "write_row",
            "write_column",
            "write_data",
            "read_row",
            "read_column",
        ]
    );
    let memory = (&model.registers)
        .into_iter()
        .find(|signal| signal.name == "memory")
        .unwrap();
    assert_eq!(memory.bits.len(), 24);
    assert_eq!(signal_names(&model.outputs), vec!["read_data"]);
    assert_eq!(
        property_names(&model.assertions),
        vec!["assert_negative_index"]
    );
    model.fsm.verify(VerifyOrdering::Verify);
    simulate_multidim_arrays(&model);
}

#[test]
fn zero_init_is_written_for_every_register() {
    let mut model = load_model("counter");
    prepare_model_for_export(&mut model);
    let first_register = model.fsm.get_latches()[0].output.index();
    model.fsm.get_latch_mut(first_register).unwrap().reset_value = Some(true);

    model.initialize_registers_to_zero();
    let contents = write_aiger_ascii(&model.fsm, AigerVersion::V1_9);
    let (round_trip, _) = read_aiger_ascii(&contents);

    assert!(
        round_trip
            .get_latches()
            .into_iter()
            .all(|latch| latch.reset_value == Some(false))
    );
}

#[test]
fn fifo_stage_fixture() {
    let model = load_model_with_reset("fifo_stage");

    assert_eq!(model.clock.domain.name, "clk");
    assert_eq!(
        signal_names(&model.inputs),
        vec!["in_valid", "in_data", "out_ack"]
    );
    assert_eq!(model.fsm.get_inputs().len(), 6);
    let memory = (&model.registers)
        .into_iter()
        .find(|signal| signal.name == "memory")
        .unwrap();
    assert_eq!(memory.bits.len(), 16);
    let mut outputs = signal_names(&model.outputs);
    outputs.sort();
    assert_eq!(outputs, vec!["in_ack", "out_data", "out_valid"]);
    assert_eq!(model.fsm.get_outputs().len(), 6);
    let mut assumptions = property_names(&model.assumptions);
    assumptions.sort();
    assert_eq!(assumptions, vec!["assume_input_stable_while_waiting"]);
    assert_eq!(model.assertions.len(), 9);
    assert_eq!(model.covers.len(), 2);
    model.fsm.verify(VerifyOrdering::Verify);
    verify_with_ric3(&model, &[]);
}

#[test]
fn fifo_stage_bypass_fixture() {
    let model = load_model_with_reset("fifo_stage_bypass");

    assert_eq!(model.clock.domain.name, "clk");
    assert_eq!(
        signal_names(&model.inputs),
        vec!["in_valid", "in_data", "out_ack"]
    );
    assert_eq!(model.fsm.get_inputs().len(), 3);
    let mut outputs = signal_names(&model.outputs);
    outputs.sort();
    assert_eq!(outputs, vec!["in_ack", "out_data", "out_valid"]);
    assert_eq!(model.fsm.get_outputs().len(), 3);
    let assertions = property_names(&model.assertions);
    assert!(assertions.contains(&"assert_stable_while_stalled"));
    assert!(assertions.contains(&"assert_empty_has_no_output"));
    assert!(assertions.contains(&"assert_reference_does_not_overflow"));
    assert!(assertions.contains(&"assert_reference_does_not_underflow"));
    assert!(assertions.contains(&"assert_output_matches_reference"));
    assert_eq!(model.covers.len(), 2);
    model.fsm.verify(VerifyOrdering::Verify);
    simulate_fifo_stage_bypass(&model);
}

#[test]
fn fifo_stage_fail_assert_fixture() {
    let model = load_model_with_reset("fifo_stage_fail_assert");

    assert_eq!(model.assertions.len(), 10);
    assert!(property_names(&model.assertions).contains(&"assert_fifo_never_fills"));
    model.fsm.verify(VerifyOrdering::Verify);
    simulate_fifo_stage_assert_failure(&model);
    verify_with_ric3(&model, &["assert_fifo_never_fills"]);
}

#[test]
fn fifo_stage_fail_free_fixture() {
    let model = load_model_with_reset("fifo_stage_fail_free");

    assert!(signal_names(&model.inputs).contains(&"undriven_output_mask"));
    assert_eq!(model.assertions.len(), 9);
    assert!(property_names(&model.assertions).contains(&"assert_fifo_order"));
    model.fsm.verify(VerifyOrdering::Verify);
    simulate_fifo_stage_free_failure(&model);
    verify_with_ric3(&model, &["assert_fifo_order"]);
}

#[test]
fn packet_switch_fixture() {
    let mut model = load_model_with_reset("packet_switch");

    assert_eq!(model.clock.domain.name, "clk");
    assert_eq!(
        signal_names(&model.inputs),
        vec![
            "in0_valid",
            "in0_dest",
            "in0_data",
            "in1_valid",
            "in1_dest",
            "in1_data",
            "out0_ack",
            "out1_ack",
        ]
    );
    assert_eq!(model.fsm.get_inputs().len(), 14);
    let mut outputs = signal_names(&model.outputs);
    outputs.sort();
    assert_eq!(
        outputs,
        vec![
            "in0_ack",
            "in1_ack",
            "out0_data",
            "out0_valid",
            "out1_data",
            "out1_valid",
        ]
    );
    assert_eq!(model.fsm.get_outputs().len(), 12);

    let mut assumptions = property_names(&model.assumptions);
    assumptions.sort();
    assert_eq!(
        assumptions,
        vec![
            "assume_input0_stable_while_waiting",
            "assume_input1_stable_while_waiting",
        ]
    );
    assert_eq!(model.assertions.len(), 31);
    assert_eq!(model.covers.len(), 5);
    assert!(property_names(&model.assertions).contains(&"assert_output0_uses_reference_data"));
    assert!(property_names(&model.assertions).contains(&"assert_rr1_prefers_loser"));
    assert!(property_names(&model.assertions).contains(&"assert_reset_empties_queues"));
    assert!(property_names(&model.assertions).contains(&"assert_input0_capacity_strengthened"));
    assert!(property_names(&model.assertions).contains(&"assert_input1_capacity_strengthened"));
    assert!(property_names(&model.covers).contains(&"cover_full_capacity"));
    assert!(property_names(&model.covers).contains(&"cover_parallel_outputs"));

    model.fsm.verify(VerifyOrdering::Verify);
    simulate_packet_switch(&model);
    verify_with_ric3(&model, &[]);
    verify_packet_switch_aag_export(&mut model);
}

static NEXT_TEMP_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(fixture: &str) -> Self {
        let sequence = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "formal-ric3-{}-{sequence}-{fixture}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap_or_else(|error| {
            panic!(
                "failed to create ric3 temporary directory {}: {error}",
                path.display()
            )
        });
        Self(path)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!(
                "failed to remove ric3 temporary directory {}: {error}",
                self.0.display()
            );
        }
    }
}

fn verify_with_ric3(model: &NamedFsm, expected_sat_assertions: &[&str]) {
    let fixture = model
        .clock
        .domain
        .name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .collect::<String>();
    let temporary_directory = TempDirectory::new(&fixture);
    let mut prepared_fsm = model.fsm.clone();
    prepared_fsm.normalize_outputs();
    prepared_fsm.reorder_gates();
    prepared_fsm.verify(VerifyOrdering::Verify);

    for expected_name in expected_sat_assertions {
        assert!(
            (&model.assertions)
                .into_iter()
                .any(|property| property.name == *expected_name),
            "missing assertion expected to be SAT: {expected_name}"
        );
    }

    for (index, property) in (&model.assertions).into_iter().enumerate() {
        let assertion = prepared_fsm.get_asserts()[index].clone();
        let expected = if expected_sat_assertions.contains(&property.name.as_str()) {
            "SAT"
        } else {
            "UNSAT"
        };
        verify_property_with_ric3(
            &prepared_fsm,
            assertion,
            &temporary_directory.0.join(format!("assertion-{index}.aig")),
            &property.name,
            expected,
        );
    }

    for (index, property) in (&model.covers).into_iter().enumerate() {
        let (value, label) = prepared_fsm.get_covers()[index].clone();
        verify_property_with_ric3(
            &prepared_fsm,
            (!value, label),
            &temporary_directory.0.join(format!("cover-{index}.aig")),
            &property.name,
            "SAT",
        );
    }
}

fn verify_property_with_ric3(
    prepared_fsm: &FSM,
    assertion: (formal_utils::value::Value, Option<String>),
    path: &Path,
    property_name: &str,
    expected: &str,
) {
    let mut fsm = prepared_fsm.clone();
    fsm.get_asserts_mut().clear();
    fsm.get_asserts_mut().push(assertion);
    fsm.get_covers_mut().clear();
    fs::write(path, write_aiger_binary(&fsm, AigerVersion::V1_9))
        .unwrap_or_else(|error| panic!("failed to write ric3 model {}: {error}", path.display()));

    let start = Instant::now();
    let output = Command::new("ric3")
        .arg("check")
        .arg(path)
        .arg("ic3")
        .output()
        .unwrap_or_else(|error| panic!("failed to run ric3 for {property_name}: {error}"));
    eprintln!("ric3 checked {property_name} in {:?}", start.elapsed());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "ric3 failed for {property_name}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let result = stdout
        .lines()
        .find(|line| *line == "SAT" || *line == "UNSAT")
        .unwrap_or_else(|| {
            panic!(
                "ric3 returned no SAT/UNSAT result for {property_name}\nstdout:\n{stdout}\nstderr:\n{stderr}"
            )
        });
    assert_eq!(
        result, expected,
        "unexpected ric3 result for {property_name}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

fn verify_counter_model(model: &NamedFsm) {
    assert_eq!(model.clock.domain.name, "clk");
    assert_eq!(model.clock.domain.edge, Edge::Positive);
    assert_eq!(signal_names(&model.inputs), vec!["reset_n", "enable"]);
    assert_eq!(model.fsm.get_inputs().len(), 2);
    assert_eq!(model.fsm.get_latches().len(), 41);
    assert_eq!(signal_names(&model.outputs), vec!["count"]);
    assert_eq!(model.fsm.get_outputs().len(), 4);
    assert_eq!(
        property_names(&model.assertions),
        vec!["assert_holds_when_disabled"]
    );
    assert_eq!(
        property_names(&model.assumptions),
        vec!["assume_no_overflow"]
    );
    assert_eq!(property_names(&model.covers), vec!["cover_saturation"]);
    model.fsm.verify(VerifyOrdering::Verify);
}

fn verify_formal_history_initialization(model: &NamedFsm) {
    let latches = model.fsm.get_latches();

    for register in &model.registers {
        let name = register.name.rsplit('.').next().unwrap();
        let expected =
            (name.starts_with("_Vpast_") || name.starts_with("__Vnfa_")).then_some(false);
        for bit in &register.bits {
            let output = bit.value.unwrap_variable();
            let latch = (&latches)
                .into_iter()
                .find(|latch| latch.output.index() == output.index())
                .unwrap();
            assert_eq!(latch.reset_value, expected, "register {}", register.name);
        }
    }
}

fn simulate_counter(model: &NamedFsm) {
    let mut simulator = Simulator::from(model.fsm.clone());

    set_input(model, &mut simulator, "reset_n", false);
    set_input(model, &mut simulator, "enable", false);
    simulator.eval();
    simulator.step();
    simulator.eval();
    assert_eq!(output_word(&simulator), 0);

    set_input(model, &mut simulator, "reset_n", true);
    set_input(model, &mut simulator, "enable", true);
    simulator.eval();
    simulator.step();
    simulator.eval();
    assert_eq!(output_word(&simulator), 1);

    set_input(model, &mut simulator, "enable", false);
    simulator.eval();
    simulator.step();
    simulator.eval();
    assert_eq!(output_word(&simulator), 1);
}

fn simulate_counter_ones(model: &NamedFsm) {
    let mut simulator = Simulator::from(model.fsm.clone());

    set_input(model, &mut simulator, "reset_n", false);
    set_input(model, &mut simulator, "enable", false);
    simulator.eval();
    simulator.step();
    simulator.eval();
    assert_eq!(output_word(&simulator), 15);
    assert_eq!(simulator.get_assert(0), Some(true));

    set_input(model, &mut simulator, "reset_n", true);
    simulator.eval();
    simulator.step();
    simulator.eval();
    assert_eq!(output_word(&simulator), 15);
    assert_eq!(simulator.get_assert(0), Some(true));

    for expected in 0..16 {
        set_input(model, &mut simulator, "enable", true);
        simulator.eval();
        simulator.step();
        simulator.eval();
        assert_eq!(output_word(&simulator), expected);
        assert_eq!(simulator.get_assert(0), Some(true));

        set_input(model, &mut simulator, "enable", false);
        simulator.eval();
        simulator.step();
        simulator.eval();
        assert_eq!(output_word(&simulator), expected);
        assert_eq!(simulator.get_assert(0), Some(true));
    }
}

fn simulate_case_statements(model: &NamedFsm) {
    let mut simulator = Simulator::from(model.fsm.clone());

    for selector in 0..8 {
        for data in 0..16 {
            set_input_word(model, &mut simulator, "selector", selector);
            set_input_word(model, &mut simulator, "data", data);
            simulator.eval();
            let expected_combinational = match selector {
                0 | 4 => data ^ 0xa,
                1 => (data + 3) & 0xf,
                2 => match data & 3 {
                    0 => 4,
                    1 | 2 => 5,
                    _ => 6,
                },
                _ => 7,
            };
            assert_eq!(
                output_signal_word(model, &simulator, "combinational_decoded"),
                expected_combinational,
                "combinational selector {selector}, data {data}"
            );
            let expected_wildcard = match selector {
                4..=7 => data,
                2..=3 => (data + 1) & 0xf,
                1 => data ^ 0xf,
                _ => 0,
            };
            assert_eq!(
                output_signal_word(model, &simulator, "wildcard_decoded"),
                expected_wildcard,
                "wildcard selector {selector}, data {data}"
            );
            simulator.step();
            simulator.eval();

            let expected = match selector {
                0 | 4 => data,
                1 => (data + 1) & 0xf,
                2 => match data & 3 {
                    0 => 8,
                    1 | 2 => 9,
                    _ => 10,
                },
                _ => 15,
            };
            assert_eq!(
                output_signal_word(model, &simulator, "decoded"),
                expected,
                "selector {selector}, data {data}"
            );
        }
    }
}

fn simulate_signed_operations(model: &NamedFsm) {
    const WIDTH: usize = 4;
    const MASK: u64 = (1 << WIDTH) - 1;

    let mut simulator = Simulator::from(model.fsm.clone());
    for lhs in 0..=MASK {
        for rhs in 0..=MASK {
            set_input_word(model, &mut simulator, "lhs", lhs);
            set_input_word(model, &mut simulator, "rhs", rhs);
            simulator.eval();
            simulator.step();
            simulator.eval();

            let expected_product = (lhs * rhs) & MASK;
            assert_eq!(
                output_signal_word(model, &simulator, "captured_unsigned_product"),
                expected_product
            );
            assert_eq!(
                output_signal_word(model, &simulator, "captured_signed_product"),
                expected_product
            );
            if let Some(quotient) = lhs.checked_div(rhs) {
                assert_eq!(
                    output_signal_word(model, &simulator, "captured_unsigned_quotient"),
                    quotient
                );
            }
            let signed_lhs = if lhs & (1 << (WIDTH - 1)) == 0 {
                lhs as i64
            } else {
                lhs as i64 - (1 << WIDTH)
            };
            let signed_rhs = if rhs & (1 << (WIDTH - 1)) == 0 {
                rhs as i64
            } else {
                rhs as i64 - (1 << WIDTH)
            };
            if let Some(quotient) = signed_lhs.checked_div(signed_rhs) {
                assert_eq!(
                    output_signal_word(model, &simulator, "captured_signed_quotient"),
                    quotient as u64 & MASK
                );
            }
        }
    }
}

fn simulate_multidim_arrays(model: &NamedFsm) {
    let mut simulator = Simulator::from(model.fsm.clone());

    for row in 0..2 {
        for column in 0..3 {
            let data = 1 + row * 3 + column;
            set_input(model, &mut simulator, "write_enable", true);
            set_input_word(model, &mut simulator, "write_row", row);
            set_input_word(model, &mut simulator, "write_column", column);
            set_input_word(model, &mut simulator, "write_data", data);
            set_input_word(model, &mut simulator, "read_row", row);
            set_input_word(model, &mut simulator, "read_column", column);
            simulator.eval();
            simulator.step();
            simulator.eval();
            assert_eq!(
                output_signal_word(model, &simulator, "read_data"),
                data,
                "row {row}, column {column} immediately after write"
            );
        }
    }

    set_input(model, &mut simulator, "write_enable", false);
    for row in 0..2 {
        for column in 0..3 {
            set_input_word(model, &mut simulator, "read_row", row);
            set_input_word(model, &mut simulator, "read_column", column);
            simulator.eval();
            assert_eq!(
                output_signal_word(model, &simulator, "read_data"),
                1 + row * 3 + column,
                "row {row}, column {column} after filling the array"
            );
        }
    }

    set_input_word(model, &mut simulator, "read_column", 3);
    simulator.eval();
    assert_eq!(output_signal_word(model, &simulator, "read_data"), 0);
}

fn simulate_fifo_stage_assert_failure(model: &NamedFsm) {
    let assertion = (&model.assertions)
        .into_iter()
        .position(|property| property.name == "assert_fifo_never_fills")
        .unwrap();
    let mut simulator = Simulator::from(model.fsm.clone());

    set_input(model, &mut simulator, "in_valid", false);
    set_input(model, &mut simulator, "in_data", false);
    set_input(model, &mut simulator, "out_ack", false);
    set_input(model, &mut simulator, "in_valid", true);
    for push in 1..=3 {
        simulator.eval();
        assert_eq!(simulator.get_assert(assertion), Some(true));
        simulator.step();
        simulator.eval();
        assert_eq!(
            simulator.get_assert(assertion),
            Some(push < 3),
            "deliberate assertion after push {push}"
        );
    }
}

fn simulate_fifo_stage_free_failure(model: &NamedFsm) {
    let assertion = (&model.assertions)
        .into_iter()
        .position(|property| property.name == "assert_fifo_order")
        .unwrap();
    let mut simulator = Simulator::from(model.fsm.clone());

    set_input(model, &mut simulator, "in_valid", false);
    set_input_word(model, &mut simulator, "in_data", 0);
    set_input(model, &mut simulator, "out_ack", false);
    set_input_word(model, &mut simulator, "undriven_output_mask", 0);
    set_input(model, &mut simulator, "in_valid", true);
    simulator.eval();
    simulator.step();

    set_input(model, &mut simulator, "in_valid", false);
    for _ in 0..2 {
        simulator.eval();
        simulator.step();
    }

    set_input(model, &mut simulator, "out_ack", true);
    set_input_word(model, &mut simulator, "undriven_output_mask", 1);
    simulator.eval();
    assert_eq!(simulator.get_assert(assertion), Some(false));
}

fn simulate_fifo_stage_bypass(model: &NamedFsm) {
    let mut simulator = Simulator::from(model.fsm.clone());

    set_input(model, &mut simulator, "in_valid", false);
    set_input_word(model, &mut simulator, "in_data", 0);
    set_input(model, &mut simulator, "out_ack", false);
    set_input(model, &mut simulator, "in_valid", true);
    set_input(model, &mut simulator, "out_ack", false);
    let mut next_input = 1;
    let mut accepted = Vec::new();
    set_input_word(model, &mut simulator, "in_data", next_input & 1);

    let mut reached_backpressure = false;
    for _ in 0..16 {
        simulator.eval();
        if output_signal_word(model, &simulator, "out_valid") == 1 {
            assert_eq!(
                output_signal_word(model, &simulator, "out_data"),
                accepted[0]
            );
        }
        verify_all_assertions(model, &simulator);

        if output_signal_word(model, &simulator, "in_ack") == 0 {
            reached_backpressure = true;
            break;
        }

        accepted.push(next_input & 1);
        simulator.step();
        next_input += 1;
        set_input_word(model, &mut simulator, "in_data", next_input & 1);
    }

    assert!(reached_backpressure);
    assert_eq!(accepted.len(), 6);
    set_input(model, &mut simulator, "out_ack", true);

    let mut drained = 0;
    let mut accepted_waiting_input = false;
    for _ in 0..16 {
        simulator.eval();
        if output_signal_word(model, &simulator, "out_valid") == 1 {
            assert_eq!(
                output_signal_word(model, &simulator, "out_data"),
                accepted[drained]
            );
            drained += 1;
        }
        verify_all_assertions(model, &simulator);

        if output_signal_word(model, &simulator, "in_ack") == 1 {
            accepted.push(next_input & 1);
            simulator.step();
            set_input(model, &mut simulator, "in_valid", false);
            accepted_waiting_input = true;
            break;
        }
        simulator.step();
    }
    assert!(accepted_waiting_input);

    for _ in 0..32 {
        if drained == accepted.len() {
            break;
        }
        simulator.eval();
        if output_signal_word(model, &simulator, "out_valid") == 1 {
            assert_eq!(
                output_signal_word(model, &simulator, "out_data"),
                accepted[drained]
            );
            drained += 1;
        }
        verify_all_assertions(model, &simulator);
        simulator.step();
    }
    assert_eq!(drained, accepted.len());

    simulator.eval();
    assert_eq!(output_signal_word(model, &simulator, "out_valid"), 0);
    verify_all_assertions(model, &simulator);

    next_input += 1;
    set_input(model, &mut simulator, "in_valid", true);
    set_input_word(model, &mut simulator, "in_data", next_input & 1);
    while accepted.len() < 20 {
        simulator.eval();
        if output_signal_word(model, &simulator, "out_valid") == 1 {
            assert_eq!(
                output_signal_word(model, &simulator, "out_data"),
                accepted[drained]
            );
            drained += 1;
        }
        verify_all_assertions(model, &simulator);

        if output_signal_word(model, &simulator, "in_ack") == 1 {
            accepted.push(next_input & 1);
            next_input += 1;
        }
        simulator.step();
        set_input_word(model, &mut simulator, "in_data", next_input & 1);
    }

    set_input(model, &mut simulator, "in_valid", false);
    for _ in 0..32 {
        if drained == accepted.len() {
            break;
        }
        simulator.eval();
        if output_signal_word(model, &simulator, "out_valid") == 1 {
            assert_eq!(
                output_signal_word(model, &simulator, "out_data"),
                accepted[drained]
            );
            drained += 1;
        }
        verify_all_assertions(model, &simulator);
        simulator.step();
    }
    assert_eq!(drained, accepted.len());
}

fn simulate_packet_switch(model: &NamedFsm) {
    let mut simulator = Simulator::from(model.fsm.clone());

    set_input(model, &mut simulator, "in0_valid", false);
    set_input(model, &mut simulator, "in0_dest", false);
    set_input_word(model, &mut simulator, "in0_data", 0);
    set_input(model, &mut simulator, "in1_valid", false);
    set_input(model, &mut simulator, "in1_dest", false);
    set_input_word(model, &mut simulator, "in1_data", 0);
    set_input(model, &mut simulator, "out0_ack", false);
    set_input(model, &mut simulator, "out1_ack", false);
    set_input(model, &mut simulator, "in0_valid", true);
    set_input(model, &mut simulator, "in1_valid", true);
    for packet in 0..3 {
        set_input_word(model, &mut simulator, "in0_data", packet + 1);
        set_input_word(model, &mut simulator, "in1_data", packet + 9);
        simulator.eval();
        assert_eq!(output_signal_word(model, &simulator, "in0_ack"), 1);
        assert_eq!(output_signal_word(model, &simulator, "in1_ack"), 1);
        step_packet_switch(model, &mut simulator);
    }

    set_input(model, &mut simulator, "in1_valid", false);
    set_input_word(model, &mut simulator, "in0_data", 4);
    set_input(model, &mut simulator, "in0_dest", true);
    set_input(model, &mut simulator, "out0_ack", true);
    simulator.eval();
    assert_eq!(output_signal_word(model, &simulator, "out0_valid"), 1);
    assert_eq!(output_signal_word(model, &simulator, "out0_data"), 1);
    assert_eq!(output_signal_word(model, &simulator, "in0_ack"), 1);
    assert_eq!(output_signal_word(model, &simulator, "in1_ack"), 0);
    step_packet_switch(model, &mut simulator);

    set_input(model, &mut simulator, "in0_valid", false);
    for expected in [9, 2, 10, 3, 11] {
        simulator.eval();
        assert_eq!(output_signal_word(model, &simulator, "out0_valid"), 1);
        assert_eq!(output_signal_word(model, &simulator, "out0_data"), expected);
        step_packet_switch(model, &mut simulator);
    }

    set_input(model, &mut simulator, "out0_ack", false);
    for _ in 0..2 {
        simulator.eval();
        assert_eq!(output_signal_word(model, &simulator, "out1_valid"), 1);
        assert_eq!(output_signal_word(model, &simulator, "out1_data"), 4);
        step_packet_switch(model, &mut simulator);
    }

    set_input(model, &mut simulator, "in1_valid", true);
    set_input(model, &mut simulator, "in1_dest", false);
    set_input_word(model, &mut simulator, "in1_data", 12);
    step_packet_switch(model, &mut simulator);

    set_input(model, &mut simulator, "in1_valid", false);
    set_input(model, &mut simulator, "out0_ack", true);
    set_input(model, &mut simulator, "out1_ack", true);
    simulator.eval();
    assert_eq!(output_signal_word(model, &simulator, "out0_valid"), 1);
    assert_eq!(output_signal_word(model, &simulator, "out0_data"), 12);
    assert_eq!(output_signal_word(model, &simulator, "out1_valid"), 1);
    assert_eq!(output_signal_word(model, &simulator, "out1_data"), 4);
    step_packet_switch(model, &mut simulator);

    assert_eq!(output_signal_word(model, &simulator, "out0_valid"), 0);
    assert_eq!(output_signal_word(model, &simulator, "out1_valid"), 0);
}

fn step_packet_switch(model: &NamedFsm, simulator: &mut Simulator) {
    simulator.eval();
    verify_all_assertions(model, simulator);
    simulator.step();
    simulator.eval();
    verify_all_assertions(model, simulator);
}

fn verify_all_assertions(model: &NamedFsm, simulator: &Simulator) {
    for (index, property) in (&model.assertions).into_iter().enumerate() {
        assert_eq!(
            simulator.get_assert(index),
            Some(true),
            "assertion {}",
            property.name
        );
    }
}

fn output_signal_word(model: &NamedFsm, simulator: &Simulator, name: &str) -> u64 {
    let mut output_index = 0;
    for signal in &model.outputs {
        if signal.name == name {
            return (0..signal.bits.len()).fold(0, |word, position| {
                word | ((simulator.get_output(output_index + position).unwrap() as u64) << position)
            });
        }
        output_index += signal.bits.len();
    }
    panic!("missing output signal {name}");
}

fn verify_packet_switch_aag_export(model: &mut NamedFsm) {
    prepare_model_for_export(model);
    let contents = write_aiger_ascii(&model.fsm, AigerVersion::V1_9);
    let text = String::from_utf8(contents.clone()).unwrap();
    let (round_trip, version) = read_aiger_ascii(&contents);

    assert_eq!(version, AigerVersion::V1_9);
    assert_eq!(round_trip.get_inputs().len(), 14);
    assert_eq!(round_trip.get_outputs().len(), 12);
    assert_eq!(round_trip.get_asserts().len(), 31);
    assert_eq!(round_trip.get_assumes().len(), 2);
    assert!(round_trip.get_covers().is_empty());
    assert!(text.contains("i0 in0_valid\n"));
    assert!(!text.contains(" reset_n\n"));
    assert!(text.contains(" in0_ack\n"));
    assert!(text.contains("b0 "));
    assert!(text.contains("c0 "));
}

fn verify_aag_export(model: &mut NamedFsm) {
    prepare_model_for_export(model);
    let contents = write_aiger_ascii(&model.fsm, AigerVersion::V1_9);
    let text = String::from_utf8(contents.clone()).unwrap();
    let (round_trip, version) = read_aiger_ascii(&contents);

    assert_eq!(version, AigerVersion::V1_9);
    assert_eq!(round_trip.get_inputs().len(), model.fsm.get_inputs().len());
    assert_eq!(
        round_trip.get_latches().len(),
        model.fsm.get_latches().len()
    );
    assert_eq!(
        round_trip
            .get_latches()
            .into_iter()
            .filter(|latch| latch.reset_value == Some(false))
            .count(),
        37
    );
    assert_eq!(
        round_trip
            .get_latches()
            .into_iter()
            .filter(|latch| latch.reset_value.is_none())
            .count(),
        4
    );
    assert_eq!(
        round_trip.get_outputs().len(),
        model.fsm.get_outputs().len()
    );
    assert_eq!(
        round_trip.get_asserts().len(),
        model.fsm.get_asserts().len()
    );
    assert_eq!(
        round_trip.get_assumes().len(),
        model.fsm.get_assumes().len()
    );
    assert!(round_trip.get_covers().is_empty());
    assert!(text.contains("i0 reset_n\n"));
    assert!(text.contains("i1 enable\n"));
    assert!(text.contains("l0 count[0]\n"));
    assert!(text.contains("o0 count[0]\n"));
    assert!(text.contains("b0 assert_holds_when_disabled\n"));
    assert!(text.contains("c0 assume_no_overflow\n"));

    assert_eq!(
        model
            .fsm
            .get_variable_label(model.fsm.get_inputs()[0].index()),
        &Some("reset_n".to_owned())
    );
    assert_eq!(
        model
            .fsm
            .get_variable_label(model.fsm.get_latches()[0].output.index()),
        &Some("count[0]".to_owned())
    );
    assert_eq!(model.fsm.get_output_label(0), &Some("count[0]".to_owned()));
    assert_eq!(
        model.fsm.get_assert_label(0),
        &Some("assert_holds_when_disabled".to_owned())
    );

    let mut stripped_model = model.fsm.clone();
    clear_symbols(&mut stripped_model);
    let stripped =
        String::from_utf8(write_aiger_ascii(&stripped_model, AigerVersion::V1_9)).unwrap();
    assert!(!stripped.contains("i0 reset_n\n"));
    assert!(!stripped.contains("o0 count[0]\n"));
    assert!(!stripped.contains("b0 assert_holds_when_disabled\n"));

    let stripped_binary = write_aiger_binary(&stripped_model, AigerVersion::V1_9);
    let named_binary = write_aiger_binary(&model.fsm, AigerVersion::V1_9);
    assert!(named_binary.starts_with(&stripped_binary));
    assert!(named_binary.len() > stripped_binary.len());
}

fn clear_symbols(fsm: &mut formal_utils::fsm::FSM) {
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
}

fn prepare_model_for_export(model: &mut NamedFsm) {
    model.fsm.get_covers_mut().clear();
    model.fsm.normalize_outputs();
    model.fsm.reorder_gates();
    model.fsm.verify(VerifyOrdering::Verify);
}

fn load_model(fixture: &str) -> NamedFsm {
    let document = AstDocument::from_path(build_fixture(fixture)).unwrap();
    let design = Design::try_from(&document).unwrap();
    NamedFsm::try_from(&design).unwrap()
}

fn load_model_with_reset(fixture: &str) -> NamedFsm {
    let document = AstDocument::from_path(build_fixture(fixture)).unwrap();
    let design = Design::try_from(&document).unwrap();
    NamedFsm::from_design(
        &design,
        Domain::new("clk", Edge::Positive),
        Some(Domain::new("reset_n", Edge::Negative)),
    )
    .unwrap()
}

fn build_fixture(fixture: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .join(fixture);
    run_ninja(&directory, fixture, &[]);

    match find_ast(&directory) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("fixture {fixture} is invalid, rebuilding: {error}");
            run_ninja(&directory, fixture, &["-t", "clean"]);
            run_ninja(&directory, fixture, &[]);
            find_ast(&directory).unwrap_or_else(|error| {
                panic!("fixture {fixture} is still invalid after rebuilding: {error}")
            })
        }
    }
}

fn run_ninja(directory: &Path, fixture: &str, arguments: &[&str]) {
    let status = Command::new("ninja")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .status()
        .unwrap_or_else(|error| panic!("failed to run Ninja for fixture {fixture}: {error}"));
    assert!(status.success(), "failed to build fixture {fixture}");
}

fn find_ast(directory: &Path) -> Result<PathBuf, String> {
    let path = directory.join("build/ast.json");
    AstDocument::from_path(&path)
        .map_err(|error| format!("AST {} is invalid: {error}", path.display()))?;
    Ok(path)
}

fn signal_names(signals: &[NamedSignal]) -> Vec<&str> {
    signals
        .into_iter()
        .map(|signal| signal.name.as_str())
        .collect()
}

fn property_names(properties: &[NamedProperty]) -> Vec<&str> {
    properties
        .into_iter()
        .map(|property| property.name.as_str())
        .collect()
}

fn set_input(model: &NamedFsm, simulator: &mut Simulator, name: &str, value: bool) {
    let signal = (&model.inputs)
        .into_iter()
        .find(|signal| signal.name == name)
        .unwrap();
    let variable = signal.bits[0].value.unwrap_variable();
    simulator.set_input(variable.index(), Some(value));
}

fn set_input_word(model: &NamedFsm, simulator: &mut Simulator, name: &str, value: u64) {
    let signal = (&model.inputs)
        .into_iter()
        .find(|signal| signal.name == name)
        .unwrap();
    for (position, bit) in (&signal.bits).into_iter().enumerate() {
        simulator.set_input(
            bit.value.unwrap_variable().index(),
            Some(value & (1 << position) != 0),
        );
    }
}

fn output_word(simulator: &Simulator) -> u64 {
    (0..simulator.get_fsm().get_outputs().len()).fold(0, |word, index| {
        word | ((simulator.get_output(index).unwrap() as u64) << index)
    })
}

#[test]
fn unlowered_nonblocking_assignments_preserve_scheduling() {
    let model = load_model("nba_semantics");
    // Columns: a, b, pipeline, packed_value, temp_result, mem0, mem1,
    // blocking_count, lane0, lane1, after each clock edge.
    let trace = include_str!("../../tests/nba_semantics/expected.txt");
    assert_eq!(trace.lines().count(), 32);
    let mut simulator = Simulator::from(model.fsm.clone());
    assert!(!signal_names(&model.registers).contains(&"temporary"));
    assert!(!signal_names(&model.registers).contains(&"address"));
    for (step, line) in trace.lines().enumerate() {
        set_input(&model, &mut simulator, "reset_n", step != 0 && step != 17);
        set_input(&model, &mut simulator, "enable", step % 4 != 2);
        set_input(&model, &mut simulator, "index", step & 1 != 0);
        set_input_word(&model, &mut simulator, "data", ((step * 19) & 255) as u64);
        simulator.eval();
        simulator.step();
        simulator.eval();
        for (name, expected) in [
            "a",
            "b",
            "pipeline",
            "packed_value",
            "temp_result",
            "mem0",
            "mem1",
            "blocking_count",
            "lane0",
            "lane1",
        ]
        .into_iter()
        .zip(line.split_whitespace())
        {
            assert_eq!(
                output_signal_word(&model, &simulator, name),
                expected.parse::<u64>().unwrap(),
                "step {step}, output {name}"
            );
        }
        verify_all_assertions(&model, &simulator);
    }
}
