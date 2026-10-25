//! What an entry point's interface must hold, completed when a module is finished.
//!
//! An entry point is written before its function body, and a translation declares variables as the
//! body needs them - a texture the first time one is sampled - so a list written up front misses
//! whatever came later. From SPIR-V 1.4 the interface names every global variable the entry point
//! reaches, below 1.4 every input and output; an omission is invalid and a driver need not say so.
//! Rather than every caller collecting its variables in time, the builder adds each one missing as
//! it finishes, the same way it holds the one-declaration rule for types.
//!
//! A fragment entry point's integer and 64-bit float inputs cannot be interpolated, and Vulkan
//! requires each decorated `Flat` (`VUID-StandaloneSpirv-Flat-04744`), built-ins among them: the
//! builder decorates any that lacks it, so a module's flatness does not depend on the declaring
//! code knowing its stage.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Instructions, VERSION_1_4, decoration, encode, execution, op, storage};

/// The `Function` storage class, which no interface names.
const FUNCTION_STORAGE: u32 = 7;

/// Entry points with every variable their interface must name, and the `Flat` decorations a
/// fragment entry point's inputs need beyond those `annotations` holds.
pub(crate) fn complete(
    version: u32,
    entry_points: &[u32],
    annotations: &[u32],
    declarations: &[u32],
) -> (Vec<u32>, Vec<u32>) {
    // Global variables in declaration order, with their storage class and pointee type.
    let mut pointers: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
    let mut types: BTreeMap<u32, (u16, Vec<u32>)> = BTreeMap::new();
    let mut variables: Vec<(u32, u32, u32)> = Vec::new();
    for (opcode, operands) in Instructions::new(declarations) {
        match (opcode, operands) {
            (op::TYPE_POINTER, &[result, class, pointee, ..]) => {
                pointers.insert(result, (class, pointee));
            }
            (op::VARIABLE, &[pointer_type, result, class, ..]) if class != FUNCTION_STORAGE => {
                let to = pointers.get(&pointer_type).map_or(0, |&(_, to)| to);
                variables.push((result, class, to));
            }
            (
                op::TYPE_INT | op::TYPE_FLOAT | op::TYPE_VECTOR | op::TYPE_ARRAY,
                [result, rest @ ..],
            ) => {
                types.insert(*result, (opcode, rest.to_vec()));
            }
            _ => {}
        }
    }
    let flat: BTreeSet<u32> = Instructions::new(annotations)
        .filter_map(|(opcode, operands)| match (opcode, operands) {
            (op::DECORATE, &[target, decoration::FLAT, ..]) => Some(target),
            _ => None,
        })
        .collect();

    let named =
        |class: u32| version >= VERSION_1_4 || matches!(class, storage::INPUT | storage::OUTPUT);
    let mut completed = Vec::with_capacity(entry_points.len());
    let mut decorated = BTreeSet::new();
    for (opcode, operands) in Instructions::new(entry_points) {
        if opcode != op::ENTRY_POINT || operands.len() < 3 {
            encode(&mut completed, opcode, operands);
            continue;
        }
        // The name runs to the first word holding its terminating zero byte.
        let name_end = operands[2..]
            .iter()
            .position(|word| word.to_le_bytes().contains(&0))
            .map_or(operands.len(), |at| at + 3);
        let mut entry = operands.to_vec();
        let mut listed: BTreeSet<u32> = operands[name_end..].iter().copied().collect();
        for &(variable, class, _) in &variables {
            if named(class) && listed.insert(variable) {
                entry.push(variable);
            }
        }
        encode(&mut completed, opcode, &entry);
        if operands[0] == execution::FRAGMENT {
            for &(variable, class, pointee) in &variables {
                if class == storage::INPUT
                    && listed.contains(&variable)
                    && !flat.contains(&variable)
                    && cannot_interpolate(&types, pointee)
                {
                    decorated.insert(variable);
                }
            }
        }
    }
    let mut annotations = Vec::new();
    for variable in decorated {
        encode(
            &mut annotations,
            op::DECORATE,
            &[variable, decoration::FLAT],
        );
    }
    (completed, annotations)
}

/// Whether a value of type `id` is an integer or a 64-bit float, or a vector or array of them:
/// what a rasteriser cannot interpolate.
fn cannot_interpolate(types: &BTreeMap<u32, (u16, Vec<u32>)>, id: u32) -> bool {
    match types.get(&id) {
        Some((op::TYPE_INT, _)) => true,
        Some((op::TYPE_FLOAT, width)) => width.first() == Some(&64),
        Some((op::TYPE_VECTOR | op::TYPE_ARRAY, operands)) => operands
            .first()
            .is_some_and(|&element| cannot_interpolate(types, element)),
        _ => false,
    }
}
