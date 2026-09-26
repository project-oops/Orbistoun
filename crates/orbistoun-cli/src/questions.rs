//! The open-question queue: `questions`.

use crate::common::calls_by_function;

/// `worklist` - rank what to implement next, across every run so far.
/// One thing this project has written down that it does not know.
#[derive(serde::Serialize)]
struct OpenQuestion {
    /// The function it is about - a name, or a hash where there is no name yet.
    function: String,
    /// Which library it belongs to.
    library: String,
    /// The question itself, as recorded.
    question: String,
    /// How many times guests have called it across every run so far.
    ///
    /// **The ranking.** A question about a function called nine hundred times is worth
    /// more than one about a function nothing has reached, and without this the queue is
    /// alphabetical - which is the same as unordered.
    calls: u64,
    /// How many titles called it.
    modules: usize,
    /// What the function hands back, where that is established.
    ///
    /// Carried because it is the dispatch key for a property: everything returning a
    /// handle can be asked the same questions, and so can everything returning a count.
    /// A probe can generate tests from the shape without knowing the function.
    #[serde(skip_serializing_if = "Option::is_none")]
    returns: Option<String>,
    /// How many integer arguments it takes, where that is established.
    #[serde(skip_serializing_if = "Option::is_none")]
    arity: Option<u8>,
    /// What it currently rests on, so an answer can be seen to upgrade it.
    #[serde(skip_serializing_if = "Option::is_none")]
    known_by: Option<String>,
}

/// `questions` - every open question, ranked by how often a guest calls the function.
pub(crate) fn cmd_questions(top: Option<usize>, json: bool, premises: bool) {
    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
    let called = calls_by_function();

    let mut queue: Vec<OpenQuestion> = Vec::new();
    for f in knowledge.functions() {
        // Asked of the entry rather than assembled here. This shim used to apply the rule
        // itself - items, plus one for a silent guess - while `open_questions` applied a
        // different one, so `knows` reported 80 and this reported 70 of the same knowledge
        // base and neither said which it meant (D239).
        let asked = f.open_questions_asked();
        let (calls, modules) = called.get(&f.name).copied().unwrap_or((0, 0));
        for question in asked {
            queue.push(OpenQuestion {
                function: f.name.clone(),
                library: knowledge.library_of(&f.name).unwrap_or("?").to_owned(),
                question,
                calls,
                modules,
                returns: f.returns.map(|r| format!("{r:?}").to_lowercase()),
                arity: f.arity,
                known_by: f.known_by.map(|k| k.label().to_owned()),
            });
        }
    }
    // Most-called first; then by name so the order is total and a diff means something.
    queue.sort_by(|a, b| {
        b.calls
            .cmp(&a.calls)
            .then_with(|| a.function.cmp(&b.function))
            .then_with(|| a.question.cmp(&b.question))
    });
    // **Before `top` truncates.** A premise is the set of entries resting on it, so
    // grouping a shortened queue answers a different question with the same words: it
    // would report that four functions share something fourteen of them share, and say so
    // as confidently as the full run does.
    if premises {
        print_premises(&queue, json, top);
        return;
    }

    if let Some(n) = top {
        queue.truncate(n);
    }

    if json {
        match serde_json::to_string_pretty(&queue) {
            Ok(text) => println!("{text}"),
            Err(e) => eprintln!("could not render the queue: {e}"),
        }
        return;
    }

    println!(
        "{} open questions, ranked by how often a guest calls the function",
        queue.len()
    );
    println!();
    let mut last = String::new();
    for q in &queue {
        if q.function != last {
            let shape = match (q.returns.as_deref(), q.arity) {
                (Some(r), Some(a)) => format!("returns {r}, {a} args"),
                (Some(r), None) => format!("returns {r}"),
                (None, Some(a)) => format!("{a} args"),
                (None, None) => "shape unrecorded".to_owned(),
            };
            println!(
                "  {:>10} calls in {} module(s)   {}::{}   [{shape}]",
                q.calls, q.modules, q.library, q.function
            );
            last.clone_from(&q.function);
        }
        println!("      ? {}", q.question);
    }
}

/// Print the queue grouped by the premise its entries share.
///
/// # Why this is worth a mode of its own
///
/// The per-function listing is the right shape for "what is unknown about this function"
/// and the wrong shape for "what would a console sweep have to establish". Most of the
/// queue is one sentence repeated: the entries resting on the commonest premise are a
/// large fraction of the whole list, and read one at a time they look like that many
/// separate asks. Grouped, a probe can sample a premise instead of enumerating it.
///
/// **Ranked by the calls behind the premise, not by how many entries carry it.** A premise
/// shared by a hundred functions nothing ever calls is worth less than one shared by two
/// that a guest is in constantly, and calls are the ranking this command already uses.
fn print_premises(queue: &[OpenQuestion], json: bool, top: Option<usize>) {
    let asked: Vec<(String, String)> = queue
        .iter()
        .map(|q| (q.function.clone(), q.question.clone()))
        .collect();
    // Calls are per function, and a function may rest on several premises - so a premise
    // is credited with the calls of each function under it, and the totals across premises
    // deliberately sum to more than the number of calls made.
    let calls: std::collections::HashMap<&str, u64> = queue
        .iter()
        .map(|q| (q.function.as_str(), q.calls))
        .collect();
    let library: std::collections::HashMap<&str, &str> = queue
        .iter()
        .map(|q| (q.function.as_str(), q.library.as_str()))
        .collect();

    let mut grouped: Vec<(u64, orbistoun_hle::knowledge::SharedPremise)> =
        orbistoun_hle::knowledge::shared_premises(&asked)
            .into_iter()
            .map(|p| {
                let total = p
                    .functions
                    .iter()
                    .map(|f| calls.get(f.as_str()).copied().unwrap_or(0))
                    .sum();
                (total, p)
            })
            .collect();
    grouped.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.functions.len().cmp(&a.1.functions.len()))
            .then_with(|| a.1.question.cmp(&b.1.question))
    });

    let rendered: Vec<SharedAsk> = grouped
        .iter()
        .map(|(total, premise)| {
            let mut libraries: Vec<String> = premise
                .functions
                .iter()
                .filter_map(|f| library.get(f.as_str()).map(|l| (*l).to_owned()))
                .collect();
            libraries.sort_unstable();
            libraries.dedup();
            // Named, because a sweep has to pick which of them to sample.
            let mut functions = premise.functions.clone();
            functions.sort_unstable();
            functions.dedup();
            SharedAsk {
                question: premise.question.clone(),
                functions,
                libraries,
                calls: *total,
            }
        })
        .collect();

    if json {
        let shown = top.unwrap_or(rendered.len()).min(rendered.len());
        match serde_json::to_string_pretty(&rendered[..shown]) {
            Ok(text) => println!("{text}"),
            Err(e) => eprintln!("could not render the premises: {e}"),
        }
        return;
    }

    let shown = top.unwrap_or(rendered.len()).min(rendered.len());
    let shared = rendered.iter().filter(|a| a.functions.len() > 1).count();
    let covered: usize = rendered
        .iter()
        .filter(|a| a.functions.len() > 1)
        .map(|a| a.functions.len())
        .sum();
    println!(
        "{} premises behind {} open questions - and {shared} of them carry {covered} of it",
        rendered.len(),
        queue.len()
    );
    if shown < rendered.len() {
        println!("showing the {shown} with the most calls behind them");
    }
    println!();
    for ask in rendered.iter().take(shown) {
        println!(
            "  {:>10} calls   {} {} across {} librar{}",
            ask.calls,
            ask.functions.len(),
            if ask.functions.len() == 1 {
                "function"
            } else {
                "functions"
            },
            ask.libraries.len(),
            if ask.libraries.len() == 1 { "y" } else { "ies" }
        );
        println!("      ? {}", ask.question);
        println!("        {}", ask.functions.join(", "));
        println!();
    }
}

/// One premise, and everything resting on it - what `questions --premises` emits.
#[derive(serde::Serialize)]
struct SharedAsk {
    /// The question, in the wording its entries share.
    question: String,
    /// Every entry asking it, so a sweep can choose which to sample.
    functions: Vec<String>,
    /// Which libraries those span. A premise crossing two is the more interesting kind.
    libraries: Vec<String>,
    /// The calls behind it - each function's total, summed.
    ///
    /// A function resting on several premises is counted in each, so these deliberately
    /// sum to more than the calls a guest made. The number ranks the premise; it is not a
    /// share of anything.
    calls: u64,
}
