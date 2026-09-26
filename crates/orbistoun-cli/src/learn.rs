//! Recording and listing knowledge: `learn` and `knows`.

use crate::Learned;
use crate::common::knowledge_path;
use anyhow::{Context, Result};

/// `learn` - record something established about a guest function.
///
/// Merges rather than replaces, so a new edge case neither restates nor drops what is already
/// known.
pub(crate) fn cmd_learn(learned: &Learned) -> Result<()> {
    use orbistoun_hle::knowledge::{KnowledgeFile, Record};

    let path = knowledge_path(&learned.library);
    let mut file = match std::fs::read_to_string(&path) {
        Ok(text) => KnowledgeFile::parse(&text)
            .with_context(|| format!("parsing the existing {}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => KnowledgeFile {
            library: learned.library.clone(),
            functions: Vec::new(),
        },
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };

    // The merge rule lives in the crate that owns the format, so the loop records through the same
    // rule (D292). The shim only locates the file and reports.
    let record = Record {
        function: learned.function.clone(),
        arity: learned.arity,
        purpose: learned.purpose.clone(),
        edge_cases: learned.edges.clone(),
        found_in: learned.seen_in.clone(),
        known_by: learned.known.map(Into::into),
        cites: learned.cites.clone(),
        assumptions: learned.assumptions.clone(),
        note: learned.note.clone(),
    };
    let faults = file.merge(&record, &orbistoun_nid::today());

    // Refused rather than defaulted: `assumed` would understate real work and anything stronger
    // would overstate it (D180).
    if !faults.is_empty() {
        anyhow::bail!(
            concat!(
                "{}

Record how it is known: ",
                "--known published|measured|guest-observed|assumed
",
                "(published and measured also need --cites)"
            ),
            faults.join(
                "
"
            )
        );
    }

    if file.library.is_empty() {
        learned.library.clone_into(&mut file.library);
    }
    let text = file.render().context("rendering the knowledge file")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    println!(
        "{}: recorded {} ({} known, {} understood)",
        path.display(),
        learned.function,
        file.functions.len(),
        file.functions.iter().filter(|f| !f.is_bare()).count()
    );
    Ok(())
}

/// `knows` - print what is known about guest functions.
pub(crate) fn cmd_knows(pattern: Option<&str>) {
    /// Written as a constant so the replacement below is not a bare escape in a call.
    const NEWLINE: &str = "
";
    /// What a wrapped purpose line is prefixed with, to line up under the first.
    const PURPOSE_CONTINUATION: &str = "
            ";

    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();

    let Some(pattern) = pattern else {
        println!(
            "{} functions recorded, {} understood beyond a name",
            knowledge.len(),
            knowledge.understood()
        );
        print_provenance_summary(&knowledge);
        for f in knowledge.functions() {
            let mark = if f.is_bare() { " " } else { "*" };
            println!(
                "  {mark} {:<40} {}",
                f.name,
                knowledge.library_of(&f.name).unwrap_or("")
            );
        }
        println!("{NEWLINE}(* means something beyond the name is recorded)");
        return;
    };

    for f in knowledge.functions().filter(|f| f.name.contains(pattern)) {
        println!(
            "{NEWLINE}{}  [{}]",
            f.name,
            knowledge.library_of(&f.name).unwrap_or("?")
        );
        if let Some(arity) = f.arity {
            println!("  arity {arity}");
        }
        if !f.purpose.is_empty() {
            // Indented so a multi-line purpose reads as one block, apart from the labels beneath
            // it.
            let indented = f.purpose.trim().replace(NEWLINE, PURPOSE_CONTINUATION);
            println!("  purpose {indented}");
        }
        for (i, a) in f.arguments.iter().enumerate() {
            println!("  arg {i} {:<12} {:<8} {}", a.name, a.kind, a.note);
        }
        for edge in &f.edge_cases {
            println!("  edge {edge}");
        }
        if !f.found_by.is_empty() || !f.found_in.is_empty() {
            println!(
                "  found {} {}{}",
                f.found_by,
                if f.found_in.is_empty() {
                    String::new()
                } else {
                    format!("in {} ", f.found_in.join(", "))
                },
                f.found_on
            );
        }
        if let Some(known) = f.known_by {
            println!(
                "  known {:<14}{}",
                known.label(),
                if f.cites.is_empty() { "" } else { &f.cites }
            );
        }
        for assumption in &f.assumptions {
            println!("  assumes {assumption}");
        }
        if !f.note.is_empty() {
            println!("  note {}", f.note);
        }
    }
}

/// How the knowledge base knows what it claims, and how much of it is assumed.
///
/// Printed unprompted so the provenance field stays maintained: equal counts of assumed and
/// measured entries describe very different knowledge. The open-question total rises as assumptions
/// are written down and falls as hardware answers them.
fn print_provenance_summary(knowledge: &orbistoun_hle::knowledge::Knowledge) {
    use orbistoun_hle::knowledge::Oracle;

    let counts = [
        Oracle::Published,
        Oracle::Differential,
        Oracle::Measured,
        Oracle::GuestObserved,
        Oracle::Assumed,
    ]
    .map(|o| (o, knowledge.resting_on(o)));

    let shown: Vec<String> = counts
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(o, n)| format!("{n} {}", o.label()))
        .collect();
    if !shown.is_empty() {
        println!("  resting on {}", shown.join(", "));
    }

    let open = knowledge.open_questions();
    if open > 0 {
        println!("  {open} open questions a probe on real hardware could settle");
    }

    // Never silent on a fault: unaccounted claims would read as checked.
    let faults = knowledge.provenance_faults();
    if !faults.is_empty() {
        println!(
            "  {} entries do not account for what they claim:",
            faults.len()
        );
        for fault in faults.iter().take(10) {
            println!("    ! {fault}");
        }
    }
}
