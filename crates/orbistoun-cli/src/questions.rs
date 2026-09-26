//! The open-question queue: `questions`.

use crate::common::calls_by_function;

/// One thing the knowledge base records that it does not know.
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
    /// The ranking: a question about a heavily called function is worth more than one nothing has
    /// reached.
    calls: u64,
    /// How many titles called it.
    modules: usize,
    /// What the function hands back, where that is established.
    ///
    /// The dispatch key for a property: every function returning a handle can be asked the same
    /// questions, so a probe can generate tests from the shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    returns: Option<String>,
    /// How many integer arguments it takes, where that is established.
    #[serde(skip_serializing_if = "Option::is_none")]
    arity: Option<u8>,
    /// What it rests on, so an answer can be seen to upgrade it.
    #[serde(skip_serializing_if = "Option::is_none")]
    known_by: Option<String>,
}

/// `questions` - every open question, ranked by how often a guest calls the function.
pub(crate) fn cmd_questions(top: Option<usize>, json: bool, premises: bool) {
    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
    let called = calls_by_function();

    let mut queue: Vec<OpenQuestion> = Vec::new();
    for f in knowledge.functions() {
        // Asked of the entry so `knows` and `questions` count open questions by the same rule.
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
    // Most-called first, then by name, so the order is total and a diff is meaningful.
    queue.sort_by(|a, b| {
        b.calls
            .cmp(&a.calls)
            .then_with(|| a.function.cmp(&b.function))
            .then_with(|| a.question.cmp(&b.question))
    });
    // Grouped before `top` truncates: grouping a shortened queue would under-count the functions
    // sharing a premise (D538).
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
/// Much of the queue is one sentence repeated across a family; grouped, a probe can sample a
/// premise instead of enumerating it. Ranked by the calls behind the premise, not by how many
/// entries carry it.
fn print_premises(queue: &[OpenQuestion], json: bool, top: Option<usize>) {
    let asked: Vec<(String, String)> = queue
        .iter()
        .map(|q| (q.function.clone(), q.question.clone()))
        .collect();
    // A function may rest on several premises and is credited to each, so the totals across
    // premises sum to more than the calls made.
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
            // Listed, because a sweep picks which of them to sample.
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
    /// Which libraries those span.
    libraries: Vec<String>,
    /// The calls behind it - each function's total, summed.
    ///
    /// A function resting on several premises is counted in each, so this ranks the premise and is
    /// not a share of anything.
    calls: u64,
}
