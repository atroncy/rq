extern crate ansi_term;
extern crate docopt;
extern crate duk;
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate nix;
extern crate protobuf;
extern crate record_query;
extern crate rustc_serialize;
extern crate serde_protobuf;

use std::env;
use std::io;
use std::path;

use record_query as rq;

const VERSION: &'static str = env!("CARGO_PKG_VERSION");

pub const DOCOPT: &'static str = concat!("
rq - record query v", env!("CARGO_PKG_VERSION"), "

A tool for manipulating data records.

Records are read from stdin, processed, and written to stdout.  The tool accepts
a query in the custom rq query language as its main command-line arguments.

See https://github.com/dflemstr/rq for in-depth documentation.

Usage:
  rq (--help|--version)
  rq [-j|-c|-h|-m|-p <type>|-y] [-J|-C|-H|-M|-P <type>|-Y] [-l <spec>|-q] [--trace] [--] [<query>]
  rq [-l <spec>|-q] [--trace] protobuf add <schema> [--base <path>]

Options:
  --help
      Show this screen.
  --version
      Show the program name and version.

  -j, --input-json
      Input is white-space separated JSON values (default).
  -J, --output-json
      Output should be formatted as JSON values (default).
  -c, --input-cbor
      Input is a series of CBOR values.
  -C, --output-cbor
      Output is a series of CBOR values.
  -h, --input-hjson
      Input is a HJSON document.
  -H, --output-hjson
      Output should be formatted as HJSON values.
  -m, --input-message-pack
      Input is formatted as MessagePack.
  -M, --output-message-pack
      Output should be formatted as MessagePack values.
  -p <type>, --input-protobuf <type>
      Input is a single protocol buffer object.  The argument refers to the
      fully qualified name of the message type (including the leading '.').
  -P <type>, --output-protobuf <type>
      Output should be formatted as protocol buffer objects.  The argument
      refers to the fully qualified name of the message type (including the
      leading '.').
  -y, --input-yaml
      Input is a series of YAML documents.
  -Y, --output-yaml
      Output should be formatted as YAML documents.

  <query>
      A query indicating how to transform each record.

  --base <path>
      Directories are significant when dealing with protocol buffer
      schemas.  This specifies the base directory used to normalize schema
      file paths [default: .]

  -l <spec>, --log <spec>
      Configure logging using the supplied specification, in the format of
      `env_logger`, for example `rq=info,duk=trace/[0-9]+ objects kept`.
      See: https://doc.rust-lang.org/log/env_logger
  --trace
      Enable (back)trace output on error.
  -q, --quiet
      Log nothing.
");

#[derive(Debug, RustcDecodable)]
pub struct Args {
    pub cmd_protobuf: bool,
    pub arg_schema: String,
    pub flag_input_json: bool,
    pub flag__: bool,
    pub flag_input_protobuf: Option<String>,
    pub flag_output_protobuf: Option<String>,
    pub arg_query: String,
    pub flag_input_cbor: bool,
    pub flag_version: bool,
    pub flag_input_hjson: bool,
    pub flag_help: bool,
    pub flag_output_hjson: bool,
    pub flag_trace: bool,
    pub flag_input_yaml: bool,
    pub flag_output_cbor: bool,
    pub flag_log: Option<String>,
    pub flag_output_message_pack: bool,
    pub cmd_add: bool,
    pub flag_output_json: bool,
    pub flag_output_yaml: bool,
    pub flag_base: Option<String>,
    pub flag_quiet: bool,
    pub flag_input_message_pack: bool,
}

fn main() {
    let args: Args = docopt::Docopt::new(DOCOPT)
        .unwrap()
        .version(Some(VERSION.to_owned()))
        .decode()
        .unwrap_or_else(|e| e.exit());

    setup_log(args.flag_log.as_ref().map(String::as_ref), args.flag_quiet);

    main_with_args(&args).unwrap_or_else(|e| log_error(&args, e));
}

fn main_with_args(args: &Args) -> rq::error::Result<()> {
    let paths = rq::config::Paths::new().unwrap();

    if args.cmd_protobuf {
        if args.cmd_add {
            let schema = path::Path::new(&args.arg_schema);
            let base = path::Path::new(if let Some(ref b) = args.flag_base {
                b.as_str()
            } else {
                "."
            });
            rq::proto_index::add_file(&paths, base, schema)
        } else {
            unreachable!()
        }
    } else {
        run(&args, &paths)
    }
}

fn run(args: &Args, paths: &rq::config::Paths) -> rq::error::Result<()> {

    let stdin = io::stdin();
    let mut input = stdin.lock();

    if let Some(ref name) = args.flag_input_protobuf {
        let proto_descriptors = try!(load_descriptors(&paths));
        let stream = protobuf::CodedInputStream::new(&mut input);
        let source = try!(rq::value::protobuf::source(&proto_descriptors, name, stream));
        run_source(args, paths, source)
    } else if args.flag_input_cbor {
        let source = rq::value::cbor::source(&mut input);
        run_source(args, paths, source)
    } else if args.flag_input_message_pack {
        let source = rq::value::messagepack::source(&mut input);
        run_source(args, paths, source)
    } else if args.flag_input_hjson {
        let source = try!(rq::value::hjson::source(&mut input));
        run_source(args, paths, source)
    } else if args.flag_input_yaml {
        let source = try!(rq::value::yaml::source(&mut input));
        run_source(args, paths, source)
    } else {
        let source = rq::value::json::source(&mut input);
        run_source(args, paths, source)
    }
}

fn run_source<I>(args: &Args, paths: &rq::config::Paths, source: I) -> rq::error::Result<()>
    where I: rq::value::Source
{
    let mut output = io::stdout();

    if let Some(_) = args.flag_output_protobuf {
        Err(rq::error::Error::unimplemented("protobuf serialization".to_owned()))
    } else if args.flag_output_cbor {
        let sink = rq::value::cbor::sink(&mut output);
        run_source_sink(args, paths, source, sink)
    } else if args.flag_output_message_pack {
        let sink = rq::value::messagepack::sink(&mut output);
        run_source_sink(args, paths, source, sink)
    } else if args.flag_output_hjson {
        let sink = rq::value::hjson::sink(&mut output);
        run_source_sink(args, paths, source, sink)
    } else if args.flag_output_yaml {
        let sink = rq::value::yaml::sink(&mut output);
        run_source_sink(args, paths, source, sink)
    } else {
        let sink = rq::value::json::sink(&mut output);
        run_source_sink(args, paths, source, sink)
    }
}

fn run_source_sink<I, O>(args: &Args,
                         _paths: &rq::config::Paths,
                         source: I,
                         sink: O)
                         -> rq::error::Result<()>
    where I: rq::value::Source,
          O: rq::value::Sink
{
    let query = try!(rq::query::Query::parse(if args.arg_query.is_empty() {
        "id"
    } else {
        &args.arg_query
    }));

    record_query::run_query(&query, source, sink)
}

fn load_descriptors(paths: &rq::config::Paths)
                    -> rq::error::Result<serde_protobuf::descriptor::Descriptors> {
    let descriptors_proto = try!(rq::proto_index::compile_descriptor_set(paths));
    Ok(serde_protobuf::descriptor::Descriptors::from_proto(&descriptors_proto))
}

fn log_error(args: &Args, error: rq::error::Error) {
    use record_query::error::ErrorKind;

    match *error.kind() {
        ErrorKind::Msg(ref m) => error!("{}", m),
        ErrorKind::Duk(duk::ErrorKind::Js(ref e)) => {
            if let Some(ref stack) = e.stack {
                error!("Error while executing Javascript:");
                for line in stack.lines() {
                    error!("{}", line);
                }
            } else {
                error!("Error while executing Javascript: {}", e.message);
            }
        },
        _ => {
            let main_str = format!("{}", error);
            let mut main_lines = main_str.lines();
            error!("Encountered: {}", main_lines.next().unwrap());
            for line in main_lines {
                error!("  {}", line);
            }
            for e in error.iter().skip(1) {
                let sub_str = format!("{}", e);
                let mut sub_lines = sub_str.lines();
                error!("Caused by: {}", sub_lines.next().unwrap());
                for line in sub_lines {
                    error!("  {}", line);
                }
            }
        },
    }

    if args.flag_trace || env::var("RUST_BACKTRACE").as_ref().map(String::as_str) == Ok("1") {
        error!("");
        if let Some(backtrace) = error.backtrace() {
            error!("Backtrace:");
            for line in format!("{:?}", backtrace).lines() {
                error!("  {}", line);
            }
        } else {
            error!("(No backtrace available)");
        }
    } else {
        error!("(Re-run with --trace or RUST_BACKTRACE=1 for a backtrace)");
    }
}

fn setup_log(spec: Option<&str>, quiet: bool) {
    use log::LogLevelFilter;

    let mut builder = env_logger::LogBuilder::new();

    if quiet {
        builder.filter(None, LogLevelFilter::Off);
    } else if let Some(s) = spec {
        builder.parse(s);
    } else if let Ok(s) = env::var("RUST_LOG") {
        builder.parse(&s);
    } else {
        builder.filter(None, LogLevelFilter::Info);
    };

    builder.format(format_log_record);

    builder.init().unwrap();
}

fn format_log_record(record: &log::LogRecord) -> String {
    use ansi_term::ANSIStrings;
    use ansi_term::Colour;
    use ansi_term::Style;
    use log::LogLevel;
    use nix::unistd;
    use nix::sys::ioctl;

    if unistd::isatty(ioctl::libc::STDERR_FILENO).unwrap_or(false) {
        let normal = Style::new();
        let (front, back) = match record.level() {
            LogLevel::Error => (Colour::Red.normal(), Colour::Red.dimmed()),
            LogLevel::Warn => (Colour::Yellow.normal(), Colour::Yellow.dimmed()),
            LogLevel::Info => (Colour::Blue.normal(), Colour::Blue.dimmed()),
            LogLevel::Debug => (Colour::Purple.normal(), Colour::Purple.dimmed()),
            LogLevel::Trace => (Colour::White.dimmed(), Colour::Black.normal()),
        };

        let strings = &[back.paint("["),
                        front.paint(format!("{}", record.level())),
                        back.paint("]"),
                        normal.paint(" "),
                        back.paint("["),
                        front.paint(record.location().module_path()),
                        back.paint("]"),
                        normal.paint(" "),
                        back.paint(format!("{}", record.args()))];

        format!("{}", ANSIStrings(strings))
    } else {
        format!("[{}] [{}] {}",
                record.level(),
                record.location().module_path(),
                record.args())
    }

}

#[cfg(test)]
mod test {
    use super::*;

    use docopt;

    fn parse_args(args: &[&str]) -> Args {
        let a = docopt::Docopt::new(DOCOPT)
            .unwrap()
            .argv(args)
            .decode()
            .unwrap();
        println!("{:?}", a);
        a
    }

    #[test]
    fn test_docopt_kitchen_sink() {
        let a = parse_args(&["rq", "-l", "info", "-jP", ".foo.Bar", "select x"]);
        assert!(a.flag_input_json);
        assert_eq!(a.flag_output_protobuf, Some(".foo.Bar".to_owned()));
        assert_eq!(a.flag_log, Some("info".to_owned()));
        assert_eq!(a.arg_query, "select x");
    }

    #[test]
    fn test_docopt_no_args() {
        parse_args(&["rq"]);
    }

    #[test]
    #[should_panic(expected = "Help")]
    fn test_docopt_help() {
        parse_args(&["rq", "--help"]);
    }

    #[test]
    fn test_docopt_version() {
        let a = parse_args(&["rq", "--version"]);
        assert!(a.flag_version);
    }

    #[test]
    fn test_docopt_input_json() {
        let a = parse_args(&["rq", "-j"]);
        assert!(a.flag_input_json);
    }

    #[test]
    fn test_docopt_input_json_long() {
        let a = parse_args(&["rq", "--input-json"]);
        assert!(a.flag_input_json);
    }

    #[test]
    fn test_docopt_output_json() {
        let a = parse_args(&["rq", "-J"]);
        assert!(a.flag_output_json);
    }

    #[test]
    fn test_docopt_output_json_long() {
        let a = parse_args(&["rq", "--output-json"]);
        assert!(a.flag_output_json);
    }

    #[test]
    fn test_docopt_input_cbor() {
        let a = parse_args(&["rq", "-c"]);
        assert!(a.flag_input_cbor);
    }

    #[test]
    fn test_docopt_input_cbor_long() {
        let a = parse_args(&["rq", "--input-cbor"]);
        assert!(a.flag_input_cbor);
    }

    #[test]
    fn test_docopt_output_cbor() {
        let a = parse_args(&["rq", "-C"]);
        assert!(a.flag_output_cbor);
    }

    #[test]
    fn test_docopt_output_cbor_long() {
        let a = parse_args(&["rq", "--output-cbor"]);
        assert!(a.flag_output_cbor);
    }

    #[test]
    fn test_docopt_input_protobuf() {
        let a = parse_args(&["rq", "-p", ".foo.Bar"]);
        assert_eq!(a.flag_input_protobuf, Some(".foo.Bar".to_owned()));
    }

    #[test]
    fn test_docopt_input_protobuf_long() {
        let a = parse_args(&["rq", "--input-protobuf", ".foo.Bar"]);
        assert_eq!(a.flag_input_protobuf, Some(".foo.Bar".to_owned()));
    }

    #[test]
    fn test_docopt_output_protobuf() {
        let a = parse_args(&["rq", "-P", ".foo.Bar"]);
        assert_eq!(a.flag_output_protobuf, Some(".foo.Bar".to_owned()));
    }

    #[test]
    fn test_docopt_output_protobuf_long() {
        let a = parse_args(&["rq", "--output-protobuf", ".foo.Bar"]);
        assert_eq!(a.flag_output_protobuf, Some(".foo.Bar".to_owned()));
    }

    #[test]
    #[should_panic(expected = "NoMatch")]
    fn test_docopt_input_conflict() {
        parse_args(&["rq", "-jc"]);
    }

    #[test]
    #[should_panic(expected = "NoMatch")]
    fn test_docopt_output_conflict() {
        parse_args(&["rq", "-JC"]);
    }

    #[test]
    fn test_docopt_protobuf_add_schema() {
        let a = parse_args(&["rq", "-l", "info", "protobuf", "add", "schema.proto"]);
        assert_eq!(a.flag_log, Some("info".to_owned()));
        assert!(a.cmd_protobuf);
        assert!(a.cmd_add);
        assert_eq!(a.arg_schema, "schema.proto");
    }
}
