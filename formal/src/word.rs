use patronus::expr::{Context, Type};
use parser_verilator::ast::{DataType, DataTypeKind, Design, Domain, Variable, VariableKind};
use crate::error::ConvertError;
use crate::convert::select_domains;

pub fn design_to_transition_sys(design: &Design,
                                clock: Domain,
                                reset: Option<Domain>,
assert: Option<usize>, cover: Option<usize>) -> Result<patronus::system::TransitionSystem, ConvertError> {
    if assert.is_some() {
        todo!("btor: support assert selection");
    }
    if cover.is_some() {
        todo!("btor: support cover selection");
    }
    let (clock, reset) = select_domains(design, Some(&clock), reset.as_ref())?;


    let mut ctx = Context::default();

    for var in &design.variables {
        println!("{var:?}");
    }

    let inputs = design.variables.iter().filter(is_toplevel_input).map(|v| {
        let tpe = design.data_type(v.dtype);
        let sym = ctx.s
    })




    todo!()
}

fn is_toplevel_input(v: &&Variable) -> bool {

}

fn convert_tpe(tpe: &DataType) -> Type {
    match tpe.kind {
        DataTypeKind::Basic { .. } => {}
        DataTypeKind::Alias { .. } => {}
        DataTypeKind::Enum { .. } => {}
        DataTypeKind::PackedArray { .. } => {}
        DataTypeKind::UnpackedArray { .. } => {}
        DataTypeKind::PackedStruct { .. } => {}
        DataTypeKind::PackedUnion { .. } => {}
    }
}