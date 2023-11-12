use pest::{
    error::{Error as PestError, ErrorVariant},
    iterators::Pair,
    Parser,
};
use std::{borrow::Cow, collections::{HashSet, HashMap}};

use pest_derive::Parser;

macro_rules! allowed_rules {
    ($pair:ident, $($rule:pat),*) => {
        match $pair.as_rule() {
            $($rule)|* => (),
            rule => unreachable!("Unexpected rule {:?}\nContent: {:#?}", rule, $pair)
        }
    };
}

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct LParse;

fn print_tree(pair: Pair<Rule>, lvl: u32) {
    let indent = "   ".repeat(lvl as usize);

    print!("{} ", indent);
    println!("Rule:    {:?}", pair.as_rule());
    print!("{} ", indent);
    println!("Span:    {:?}", pair.as_span());
    print!("{} ", indent);
    println!("Text:    {}", pair.as_str());

    let p = pair.into_inner();
    let len = p.len();
    for (i, inner_pair) in p.enumerate() {
        print_tree(inner_pair, lvl + 1);
        if i < len && len != 1 {
            println!();
        }
    }
}

#[derive(Debug, Default)]
pub struct Context {
    current_number: u16,
}

type PestResult<T> = Result<T, PestError<Rule>>;

fn pairs_into_array<'a, const N: usize>(
    pair: Pair<'a, Rule>,
    msg: &str,
) -> PestResult<[Pair<'a, Rule>; N]> {
    let (required, extra) = pairs_into_array_excess(pair, msg)?;
    assert_eq!(extra.len(), 0);
    return Ok(required);
}

fn pairs_into_array_excess<'a, const N: usize>(
    pair: Pair<'a, Rule>,
    msg: &str,
) -> PestResult<([Pair<'a, Rule>; N], Vec<Pair<'a, Rule>>)> {
    let span = pair.as_span();
    let rule = pair.as_rule();
    let mut pairs: Vec<Pair<Rule>> = pair.into_inner().collect();
    if N > pairs.len() {
        return Err(custom_error(
            format!(
                "expected at least {} children, got {}. Matched {:?}",
                N,
                pairs.len(),
                rule
            ),
            span,
        ));
    }
    let extra = pairs.split_off(N);
    let required = pairs.try_into().map_err(|_| custom_error(msg, span))?;
    Ok((required, extra))
}

fn custom_error(msg: impl ToString, span: pest::Span) -> PestError<Rule> {
    PestError::new_from_span(
        pest::error::ErrorVariant::CustomError {
            message: msg.to_string(),
        },
        span,
    )
}

pub fn emit<'a>(pair: Pair<'a, Rule>, ctx: &mut Context) -> PestResult<Cow<'a, str>> {
    let val = match pair.as_rule() {
        Rule::existencial => match pair.as_str() {
            "@" => r"\forall ".into(),
            "&" => r"\exists ".into(),
            _ => unreachable!(),
        },
        Rule::arg => {
            let span = pair.as_span();
            let ([number, formula], mut rest) = pairs_into_array_excess(pair, "arg params")?;
            let number: u16 = number
                .as_str()
                .parse()
                .map_err(|e| custom_error(e, number.as_span()))?;
            let expected_number = ctx.current_number + 1;
            if number != expected_number {
                return Err(custom_error(
                    format!(
                        "Expected exercise number to be {}, got {}",
                        expected_number, number
                    ),
                    span,
                ));
            }
            ctx.current_number = expected_number;
            let formula = emit(formula, ctx)?;
            match rest.len() {
                0 => format!(r"\argument{{{}}}", formula).into(),
                1 => format!(r"\argument[{}]{{{}}}", emit(rest.remove(0), ctx)?, formula).into(),
                _ => unreachable!(),
            }
        }
        Rule::line_ext => pair.as_str().into(),
        Rule::bioperator => match pair.as_str() {
            "->" => r"\rightarrow ".into(),
            "^" => r"\land ".into(),
            "V" | "v" => r"\lor ".into(),
            _ => unreachable!(),
        },
        Rule::neg => {
            let [expr] = pairs_into_array(pair, "neg params")?;
            format!(r"\lnot {}", emit(expr, ctx)?).into()
        }
        Rule::conclusion => {
            let [exprs] = pairs_into_array(pair, "conclusion args")?;
            format!(r"\conclusion{{{}}}", emit(exprs, ctx)?).into()
        }
        Rule::subexpr => {
            let [expr] = pairs_into_array(pair, "subexpr params")?;
            format!("({})", emit(expr, ctx)?).into()
        }
        Rule::operand => pair.as_str().replace("_", r"\_").into(),
        Rule::func => {
            let ([name, args], _) = pairs_into_array_excess(pair, "func params")?;
            let args: Result<Vec<_>, _> = args.into_inner().map(|p| emit(p, ctx)).collect();
            let args = args?.join(", ");
            format!(r"\pred{{{}}}{{{}}}", emit(name, ctx)?, args).into()
        }
        Rule::header | Rule::header_line | Rule::header_key | Rule::header_value => unreachable!(),
        _ => pair
            .into_inner()
            .map(|p| emit(p, ctx))
            .collect::<Result<String, _>>()?
            .into(),
    };
    Ok(val)
}

use askama::Template;

#[derive(Template)]
#[template(path = "template.tex", escape = "none")]
struct LatexParams<'a> {
    title: &'a str,
    author: &'a str,
    date: &'a str,
    sections: Vec<Section<'a>>,
}

#[derive(Debug, Default)]
struct Section<'a> {
    label: Cow<'a, str>,
    args: Vec<Cow<'a, str>>,
    conclusion: Cow<'a, str>,
}

fn parse_header<'a>(header: Pair<'a, Rule>) -> PestResult<LatexParams> {
    assert_eq!(header.as_rule(), Rule::header);
    const CONFIG_KEY_NAME: &str = "config";
    let ([label], lines) = pairs_into_array_excess(header, "header args")?;
    let [label] = pairs_into_array(label, "header label args")?;
    let label_value = label.as_str();
    if label_value != CONFIG_KEY_NAME {
        return Err(custom_error(format!("Expected '{}', got '{}' instead.", CONFIG_KEY_NAME, label_value), label.as_span()) );
    }
    let mut keys = HashMap::<&str, &str>::new();
    for line in lines {
        assert_eq!(line.as_rule(), Rule::header_line);
        let [key, value] = pairs_into_array(line, "header_line args")?;
        match key.as_str() {
            "title" | "date" | "author" => (),
            _ => return Err(custom_error(format!("Invalid configuration key '{}'", key.as_str()), key.as_span())),
        };
        if keys.contains_key(&key.as_str()) {
            return Err(custom_error(format!("Configuration value for '{}' has already been provided", key), key.as_span()));
        }
        let key = key.as_str();
        let value = value.as_str();
        keys.insert(key, value.trim());
    }
    Ok(LatexParams {
        title: keys["title"],
        date: keys["date"],
        author: keys["author"],
        sections: vec![]
    })
}

fn parse_root<'a>(root: Pair<'a, Rule>) -> PestResult<LatexParams> {
    assert_eq!(root.as_rule(), Rule::root);
    let mut context = Context::default();
    let mut acc = vec![];
    let mut current_section = Section::default();
    let mut root = root.into_inner();
    let header = root.next().unwrap();
    let mut params = parse_header(header)?;
    for pair in root {
        // print_tree(pair.clone(), 0);
        let rule = pair.as_rule();
        if rule == Rule::label {
            current_section.label = pair.as_str().into();
            continue;
        }
        allowed_rules!(pair, Rule::arg, Rule::conclusion);
        let text = emit(pair, &mut context)?;
        match rule {
            Rule::arg => current_section.args.push(text),
            Rule::conclusion => {
                current_section.conclusion = text;
                acc.push(std::mem::take(&mut current_section));
                context = Context::default();
            }
            _ => unreachable!(),
        }
    }
    if current_section.args.len() != 0 {
        acc.push(current_section);
    }
    params.sections = acc;
    Ok(params)
}

fn parse<'a>(input: &'a str) -> PestResult<LatexParams> {
    let p = LParse::parse(Rule::root, input)?;
    let mut p = p.collect::<Vec<_>>();
    let root = p.remove(0);
    parse_root(root)
}


#[derive(clap::Parser, Debug)]
struct Cli {
    /// Input file path, usually a .txt file
    input: String,
    /// Output file path, usually a .tex file
    #[clap(default_value = "output.tex")]
    output: String,
}

fn main() {
    let cli = <Cli as clap::Parser>::parse();
    let input = std::fs::read(&cli.input)
        .map(String::from_utf8)
        .expect("Failed to read file")
        .expect("Invalid utf8");
    let params = parse(&input).unwrap_or_else(|e| panic!("{}", e));
    let mut f = std::fs::File::create(cli.output).expect("Failed to create file");
    params.write_into(&mut f).unwrap();
}
