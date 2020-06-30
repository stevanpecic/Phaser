extern crate clap;
extern crate egsphsp;
extern crate rand;
extern crate cpu_time;

use std::path::Path;
use std::error::Error;
use std::process::exit;
use std::f32;
use std::fs::File;
use clap::{App, AppSettings, SubCommand, Arg};
use egsphsp::PHSPReader;
use egsphsp::{transform, Transform, combine,sample};
use rand::Rng;
use cpu_time::ProcessTime;
use std::time::Duration;

fn floatify(s: &str) -> f32 {
    s.trim().trim_start_matches("(").trim_end_matches(")").trim().parse::<f32>().unwrap()
}

fn main() {
    let matches = App::new("phasespace")
        .version("0.0.1")
        .author("Stevan Pecic <stevan.pecic@icloud.com>")
        .about("Transform and inspect .egsphsp \
                files")
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommand(SubCommand::with_name("print")
            .about("Print the specified fields in the specified order for n (or all) records")
            .arg(Arg::with_name("fields")
                .long("field")
                .short("f")
                .takes_value(true)
                .required(true)
                .multiple(true))
            .arg(Arg::with_name("number")
                .long("number")
                .short("n")
                .takes_value(true)
                .default_value("10"))
            .arg(Arg::with_name("input")
                .takes_value(true)
                .required(true)))
        .subcommand(SubCommand::with_name("twist")
            .about("Rotate r times by a random increment")
            .arg(Arg::with_name("input")
                .takes_value(true)
                .required(true)
                .help("Input phsp file"))
            .arg(Arg::with_name("iterations")
                .short("r")
                .takes_value(true)
                .long("iterations")
                .required(true)
                .help("Number of iterations")))
        .subcommand(SubCommand::with_name("sample")
            .about("Sample particles from phase space - does not \
                    adjust weights")
            .arg(Arg::with_name("input")
                .required(true)
                .multiple(true))
            .arg(Arg::with_name("output")
                .short("o")
                .long("output")
                .takes_value(true)
                .required(true))
            .arg(Arg::with_name("seed")
                .long("seed")
                .help("Seed as an unsigned integer")
                .default_value("0")
                .required(false))
            .arg(Arg::with_name("rate")
                .default_value("10")
                .required(false)
                .long("rate")
                .takes_value(true)
                .help("Inverse sample rate - 10 means take rougly 1 out of every 10 particles")))
        .subcommand(SubCommand::with_name("info")
            .about("Basic information on phase space file")
            .arg(Arg::with_name("input").required(true))
            .arg(Arg::with_name("format")
                .default_value("human")
                .possible_values(&["human", "json"])
                .long("format")
                .takes_value(true)
                .help("Output information in json or human format")))
        .subcommand(SubCommand::with_name("combine")
            .about("Combine phase space from one or more input files into outputfile")
            .arg(Arg::with_name("input")
                .required(true)
                .multiple(true))
            .arg(Arg::with_name("output")
                .short("o")
                .long("output")
                .takes_value(true)
                .required(true))
            .arg(Arg::with_name("delete")
                .short("d")
                .long("delete")
                .help("Delete input files as they are used (no going back!)")))
        .subcommand(SubCommand::with_name("shout")
            .about("Combine phase space files from twist algorithm")
            .arg(Arg::with_name("input")
                .takes_value(true)
                .multiple(true))
            .arg(Arg::with_name("output")
                .default_value("tns.egsphsp1")
                .short("o")
                .long("output")
                .takes_value(true)))
        .subcommand(SubCommand::with_name("rotate")
            .about("Rotate by --angle radians counter clockwise around z axis")
            .arg(Arg::with_name("in-place")
                .short("i")
                .long("in-place")
                .help("Transform input file in-place"))
            .arg(Arg::with_name("angle")
                .short("a")
                .long("angle")
                .takes_value(true)
                .required(true)
                .help("Counter clockwise angle in radians to rotate around Z axis"))
            .arg(Arg::with_name("input")
                .help("Phase space file")
                .required(true))
            .arg(Arg::with_name("output")
                .help("Output file")
                .required_unless("in-place")))
        .get_matches();
    let subcommand = matches.subcommand_name().unwrap();
    let result = if subcommand == "combine" {
        // println!("combine");
        let sub_matches = matches.subcommand_matches("combine").unwrap();
        let input_paths: Vec<&Path> = sub_matches.values_of("input")
            .unwrap()
            .map(|s| Path::new(s))
            .collect();
        let output_path = Path::new(sub_matches.value_of("output").unwrap());
        println!("combine {} files into {}",
                 input_paths.len(),
                 output_path.display());
        combine(&input_paths, output_path, sub_matches.is_present("delete"))
    } else if subcommand == "print" {
        // prints the fields specified?
        let sub_matches = matches.subcommand_matches("print").unwrap();
        let input_path = Path::new(sub_matches.value_of("input").unwrap());
        let number = sub_matches.value_of("number").unwrap().parse::<usize>().unwrap();
        let fields: Vec<&str> = sub_matches.values_of("fields").unwrap().collect();
        let file = File::open(input_path).unwrap();
        let reader = PHSPReader::from(file).unwrap();
        for field in fields.iter() {
            print!("{:<16}", field);
        }
        println!("");
        for record in reader.take(number).map(|r| r.unwrap()) {
            for field in fields.iter() {
                match field {
                    &"weight" => print!("{:<16}", record.get_weight()),
                    &"energy" => print!("{:<16}", record.total_energy()),
                    &"x" => print!("{:<16}", record.x_cm),
                    &"y" => print!("{:<16}", record.y_cm),
                    &"x_cos" => print!("{:<16}", record.x_cos),
                    &"y_cos" => print!("{:<16}", record.y_cos),
                    &"produced" => print!("{:<16}", record.bremsstrahlung_or_annihilation()),
                    &"charged" => print!("{:<16}", record.charged()),
                    &"r" => print!("{:<16}", (record.x_cm * record.x_cm + record.y_cm * record.y_cm).sqrt()),
                    _ => panic!("Unknown field {}", field)
                };
            }
            println!("");
        }
        Ok(())
    } else if subcommand == "shout" {
        let sub_matches = matches.subcommand_matches("shout").unwrap();
        let input_paths: Vec<&Path> = sub_matches.values_of("input")
            .unwrap()
            .map(|s| Path::new(s))
            .collect();
        let shout_output: String = "tns_output.egsphsp1".to_string();
        let shout_output_path = Path::new(&shout_output);
        println!("combining {} files into {}",
                 input_paths.len(),
                 shout_output_path.display());
        combine(&input_paths, shout_output_path, true)
    }
    else if subcommand == "sample" {
        let sub_matches = matches.subcommand_matches("sample").unwrap();
        let input_paths: Vec<&Path> = sub_matches.values_of("input")
            .unwrap()
            .map(|s| Path::new(s))
            .collect();
        let output_path = Path::new(sub_matches.value_of("output").unwrap());
        let rate = sub_matches.value_of("rate").unwrap().parse::<u32>().unwrap();
        let seed: &[_] = &[sub_matches.value_of("seed").unwrap().parse::<usize>().unwrap()];
        println!("sample {} file into {} at 1 in {}",
                 input_paths.len(),
                 output_path.display(),
                 rate);
        sample(&input_paths, output_path, rate, seed)
    }
    else if subcommand == "info" {
        let sub_matches = matches.subcommand_matches("info").unwrap();
        let path = Path::new(sub_matches.value_of("input").unwrap());
        let reader = PHSPReader::from(File::open(path).unwrap()).unwrap();
        let header = reader.header;

        if sub_matches.value_of("format").unwrap() == "json" {
            println!("{{");
            println!("\t\"total_particles\": {},", header.total_particles);
            println!("\t\"total_photons\": {},", header.total_photons);
            println!("\t\"maximum_energy\": {},", header.max_energy);
            println!("\t\"minimum_energy\": {},", header.min_energy);
            println!("\t\"total_particles_in_source\": {}",
                     header.total_particles_in_source);
            println!("}}");
        } else {
            println!("Total particles: {}", header.total_particles);
            println!("Total photons: {}", header.total_photons);
            println!("Total electrons/positrons: {}",
                     header.total_particles - header.total_photons);
            println!("Maximum energy: {:.*} MeV", 4, header.max_energy);
            println!("Minimum energy: {:.*} MeV", 4, header.min_energy);
            println!("Incident particles from source: {:.*}",
                     1,
                     header.total_particles_in_source);
        }
        Ok(())
    } else {
        let mut matrix = [[0.0; 3]; 3];
        match subcommand {
            "rotate" =>
            {
                let sub_matches = matches.subcommand_matches("rotate").unwrap();
                let angle = floatify(sub_matches.value_of("angle").unwrap());
                Transform::rotation(&mut matrix, angle);
                let input_path = Path::new(sub_matches.value_of("input").unwrap());
                if sub_matches.is_present("in-place") {
                    println!("rotate {} by {} radians", input_path.display(), angle);
                    transform(input_path, input_path, &matrix)
                } else {
                    let output_path = Path::new(sub_matches.value_of("output").unwrap());
                    println!("rotate {} by {} radians and write to {}",
                             input_path.display(),
                             angle,
                             output_path.display());
                    transform(input_path, output_path, &matrix)
                }
            }
            "twist" =>
            {
                let start = ProcessTime::now();
                let sub_matches = matches.subcommand_matches("twist").unwrap();
                let mut rng = rand::thread_rng();
                let iteration = floatify(sub_matches.value_of("iterations").unwrap()) as i32;
                let mut count = 1 as i32;
                let input_path = Path::new(sub_matches.value_of("input").unwrap());
                loop
                {
                    let rand_seed: f32 = rng.gen();
                    let rand_angle: f32 = 6.28318 * rand_seed;
                    Transform::rotation(&mut matrix, rand_angle);
                    println!("");
                    println!("✦ Random angle is {} radians", rand_angle);
                    let mut rotation_output: String = count.to_string();
                    rotation_output.push_str(".egsphsp");
                    let rotation_output_path = Path::new(&rotation_output);
                    transform(input_path, rotation_output_path, &matrix); // Rotate file by random angle in radians & write to single_output_path
                    if count == iteration
                    {
                        println!("");
                        break
                    }
                    count = count + 1;
                }
                let cpu_time: Duration = start.elapsed();
                println!("CPU time: {:?}", cpu_time);
                Ok(())
            }
            _ => panic!("Invalid command"),
        }
    };
    match result {
        Ok(()) => exit(0),
        Err(err) => {
            println!("Error: {}", err.description());
            exit(1);
        }
    };
}
