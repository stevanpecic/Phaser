extern crate float_cmp;
extern crate byteorder;
extern crate rand;
extern crate cpu_time;

use std::error::Error;
use std::fs::{File, OpenOptions, remove_file};
use std::io::{BufReader, BufWriter};
use std::io::prelude::*;
use std::path::Path;
use std::str;
use std::io;
use std::fmt;

use cpu_time::ProcessTime;
use std::time::Duration;
use byteorder::{ByteOrder, LittleEndian};
use rand::{SeedableRng, StdRng, Rng};
use float_cmp::ApproxEqUlps;

const HEADER_LENGTH: usize = 25;
const MAX_RECORD_LENGTH: usize = 32;
const BUFFER_CAPACITY: usize = 1 * 1024 * 1024;
const MODE_LENGTH: usize = 5;

#[derive(Debug, Copy, Clone)]
pub struct Header {
    pub mode: [u8; 5],
    pub total_particles: i32,
    pub total_photons: i32,
    pub min_energy: f32,
    pub max_energy: f32,
    pub total_particles_in_source: f32,
    pub record_size: u64,
    pub using_zlast: bool,
}

#[derive(Debug, Copy, Clone)]
pub struct Record {
    pub latch: u32,
    total_energy: f32,
    pub x_cm: f32,
    pub y_cm: f32,
    pub x_cos: f32,
    pub y_cos: f32,
    pub weight: f32, // also carries the sign of the z direction, yikes
    pub zlast: Option<f32>,
}

#[derive(Debug)]
pub struct Transform;

#[derive(Debug)]
pub enum EGSError {
    Io(io::Error),
    BadMode,
    BadLength,
    ModeMismatch,
    HeaderMismatch,
    RecordMismatch,
}

pub type EGSResult<T> = Result<T, EGSError>;


impl From<io::Error> for EGSError {
    fn from(err: io::Error) -> EGSError {
        EGSError::Io(err)
    }
}

impl fmt::Display for EGSError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            EGSError::Io(ref err) => err.fmt(f),
            EGSError::BadMode => {
                write!(f,
                       "First 5 bytes of file are invalid, must be MODE0 or MODE2")
            }
            EGSError::BadLength => {
                write!(f,
                       "Number of total particles does notmatch byte length of file")
            }
            EGSError::ModeMismatch => write!(f, "Input file MODE0/MODE2 do not match"),
            EGSError::HeaderMismatch => write!(f, "Headers are different"),
            EGSError::RecordMismatch => write!(f, "Records are different"),
        }
    }
}

impl Error for EGSError {
    fn description(&self) -> &str {
        match *self {
            EGSError::Io(ref err) => err.description(),
            EGSError::BadMode => "invalid mode",
            EGSError::BadLength => "bad file length",
            EGSError::ModeMismatch => "mode mismatch",
            EGSError::HeaderMismatch => "header mismatch",
            EGSError::RecordMismatch => "record mismatch",
        }
    }

    fn cause(&self) -> Option<&dyn Error> {
        match *self {
            EGSError::Io(ref err) => Some(err),
            EGSError::BadMode => None,
            EGSError::BadLength => None,
            EGSError::ModeMismatch => None,
            EGSError::HeaderMismatch => None,
            EGSError::RecordMismatch => None,
        }
    }
}

pub struct PHSPReader {
    reader: BufReader<File>,
    pub header: Header,
    next_record: u64,
}

pub struct PHSPWriter {
    writer: BufWriter<File>,
    pub header: Header,
}


impl PHSPReader {
    pub fn from(file: File) -> EGSResult<PHSPReader> {
        let actual_size = file.metadata()?.len();
        let mut reader = BufReader::with_capacity(BUFFER_CAPACITY, file);
        let mut buffer = [0; HEADER_LENGTH];
        reader.read_exact(&mut buffer)?;
        let mut mode = [0; MODE_LENGTH];
        mode.clone_from_slice(&buffer[0..5]);
        let header = Header {
            mode: mode,
            total_particles: LittleEndian::read_i32(&buffer[5..9]),
            total_photons: LittleEndian::read_i32(&buffer[9..13]),
            max_energy: LittleEndian::read_f32(&buffer[13..17]),
            min_energy: LittleEndian::read_f32(&buffer[17..21]),
            total_particles_in_source: LittleEndian::read_f32(&buffer[21..25]),
            using_zlast: &mode == b"MODE2",
            record_size: if &mode == b"MODE0" {
                28
            } else if &mode == b"MODE2" {
                32
            } else {
                return Err(EGSError::BadMode);
            },
        };
        if actual_size != header.expected_size() as u64 {
            writeln!(&mut std::io::stderr(),
                     "Expected {} bytes in file, not {}",
                     header.expected_size(),
                     actual_size)
                .unwrap();
            //return Err(EGSError::BadLength);
        }
        reader.consume(header.record_size as usize - HEADER_LENGTH);
        Ok(PHSPReader {
            reader: reader,
            header: header,
            next_record: 0,
        })
    }
}

impl Iterator for PHSPReader {
    type Item = EGSResult<Record>;
    fn next(&mut self) -> Option<EGSResult<Record>> {
        if self.next_record >= self.header.total_particles as u64 {
            return None;
        }
        let mut buffer = [0; MAX_RECORD_LENGTH];
        match self.reader.read_exact(&mut buffer[..self.header.record_size as usize]) {
            Ok(()) => (),
            Err(err) => return Some(Err(EGSError::Io(err))),
        };
        self.next_record += 1;
        Some(Ok(Record {
            latch: LittleEndian::read_u32(&buffer[0..4]),
            total_energy: LittleEndian::read_f32(&buffer[4..8]),
            x_cm: LittleEndian::read_f32(&buffer[8..12]),
            y_cm: LittleEndian::read_f32(&buffer[12..16]),
            x_cos: LittleEndian::read_f32(&buffer[16..20]),
            y_cos: LittleEndian::read_f32(&buffer[20..24]),
            weight: LittleEndian::read_f32(&buffer[24..28]),
            zlast: if self.header.using_zlast {
                Some(LittleEndian::read_f32(&buffer[28..32]))
            } else {
                None
            },
        }))
    }
}

impl PHSPWriter {
    pub fn from(file: File, header: &Header) -> EGSResult<PHSPWriter> {
        let mut writer = BufWriter::with_capacity(BUFFER_CAPACITY, file);
        let mut buffer = [0; MAX_RECORD_LENGTH];
        buffer[0..5].clone_from_slice(&header.mode);
        LittleEndian::write_i32(&mut buffer[5..9], header.total_particles);
        LittleEndian::write_i32(&mut buffer[9..13], header.total_photons);
        LittleEndian::write_f32(&mut buffer[13..17], header.max_energy);
        LittleEndian::write_f32(&mut buffer[17..21], header.min_energy);
        LittleEndian::write_f32(&mut buffer[21..25], header.total_particles_in_source);
        writer.write_all(&buffer[..header.record_size as usize])?;
        Ok(PHSPWriter {
            header: *header,
            writer: writer,
        })
    }

    pub fn write(&mut self, record: &Record) -> EGSResult<()> {
        let mut buffer = [0; 32];
        LittleEndian::write_u32(&mut buffer[0..4], record.latch);
        LittleEndian::write_f32(&mut buffer[4..8], record.total_energy);
        LittleEndian::write_f32(&mut buffer[8..12], record.x_cm);
        LittleEndian::write_f32(&mut buffer[12..16], record.y_cm);
        LittleEndian::write_f32(&mut buffer[16..20], record.x_cos);
        LittleEndian::write_f32(&mut buffer[20..24], record.y_cos);
        LittleEndian::write_f32(&mut buffer[24..28], record.weight);
        if self.header.using_zlast {
            LittleEndian::write_f32(&mut buffer[28..32], record.weight);
        }
        self.writer.write_all(&buffer[..self.header.record_size as usize])?;
        Ok(())
    }
}

impl Header {
    fn expected_size(&self) -> usize {
        (self.total_particles as usize + 1) * self.record_size as usize
    }
    pub fn similar_to(&self, other: &Header) -> bool {
        self.mode == other.mode && self.total_particles == other.total_particles &&
        self.total_photons == other.total_photons &&
        self.max_energy.approx_eq_ulps(&other.max_energy, 10) &&
        self.min_energy.approx_eq_ulps(&other.min_energy, 10) &&
        self.total_particles_in_source.approx_eq_ulps(&other.total_particles_in_source, 2)
    }
    fn merge(&mut self, other: &Header) {
        assert!(&self.mode == &other.mode, "Merge mode mismatch");
        self.total_particles = self.total_particles
            .checked_add(other.total_particles)
            .expect("Too many particles, i32 overflow");
        self.total_photons += other.total_photons;
        self.min_energy = self.min_energy.min(other.min_energy);
        self.max_energy = self.max_energy.max(other.max_energy);
        self.total_particles_in_source += other.total_particles_in_source;
    }
}


impl Record {
    pub fn similar_to(&self, other: &Record) -> bool {
        self.latch == other.latch && self.total_energy() - other.total_energy() < 0.01 &&
        self.x_cm - other.x_cm < 0.01 && self.y_cm - other.y_cm < 0.01 &&
        self.x_cos - other.x_cos < 0.01 && self.y_cos - other.y_cos < 0.01 &&
        self.weight - other.weight < 0.01 && self.zlast == other.zlast
    }
    pub fn bremsstrahlung_or_annihilation(&self) -> bool {
        self.latch & 1 != 0
    }
    pub fn bit_region(&self) -> u32 {
        self.latch & 0xfffffe
    }
    pub fn region_number(&self) -> u32 {
        self.latch & 0xf000000
    }
    pub fn b29(&self) -> bool {
        self.latch & (1 << 29) != 0
    }
    pub fn charged(&self) -> bool {
        self.latch & (1 << 30) != 0
    }
    pub fn crossed_multiple(&self) -> bool {
        self.latch & (1 << 30) != 0
    }
    pub fn get_weight(&self) -> f32 {
        self.weight.abs()
    }
    pub fn set_weight(&mut self, new_weight: f32) {
        self.weight = new_weight * self.weight.signum();
    }
    pub fn total_energy(&self) -> f32 {
        self.total_energy.abs()
    }
    pub fn z_positive(&self) -> bool {
        self.weight.is_sign_positive()
    }
    pub fn z_cos(&self) -> f32 {
        (1.0 - (self.x_cos * self.x_cos + self.y_cos * self.y_cos)).sqrt()
    }
    pub fn first_scored_by_primary_history(&self) -> bool {
        return self.total_energy.is_sign_negative();
    }

    fn transform(&mut self, matrix: &[[f32; 3]; 3]) {
        let x_cm = self.x_cm;
        let y_cm = self.y_cm;
        self.x_cm = matrix[0][0] * x_cm + matrix[0][1] * y_cm + matrix[0][2] * 1.0;
        self.y_cm = matrix[1][0] * x_cm + matrix[1][1] * y_cm + matrix[1][2] * 1.0;
        let x_cos = self.x_cos;
        let y_cos = self.y_cos;
        self.x_cos = matrix[0][0] * x_cos + matrix[0][1] * y_cos + matrix[0][2] * self.z_cos();
        self.y_cos = matrix[1][0] * x_cos + matrix[1][1] * y_cos + matrix[1][2] * self.z_cos();
    }
}

impl Transform {
    pub fn rotation(matrix: &mut [[f32; 3]; 3], theta: f32) {
        *matrix =
            [[theta.cos(), -theta.sin(), 0.0], [theta.sin(), theta.cos(), 0.0], [0.0, 0.0, 1.0]];
    }
}



pub fn combine(input_paths: &[&Path], output_path: &Path, delete: bool) -> EGSResult<()> {
    assert!(input_paths.len() > 0, "Cannot combine zero files");
    let start = ProcessTime::now();
    let reader = PHSPReader::from(File::open(input_paths[0])?)?;
    let mut final_header = reader.header;
    for path in input_paths[1..].iter() {
        let reader = PHSPReader::from(File::open(path)?)?;
        final_header.merge(&reader.header);
    }
    println!("");
    println!("Final header: {:?}", final_header);
    println!("");
    let ofile = File::create(output_path)?;
    let mut writer = PHSPWriter::from(ofile, &final_header)?;
    for path in input_paths.iter() {
        let reader = PHSPReader::from(File::open(path)?)?;
        for record in reader {
            writer.write(&record.unwrap())?
        }
        if delete {
            remove_file(path)?;
        }
    }
    let cpu_time: Duration = start.elapsed();
    println!("CPU time: {:?}", cpu_time);
    Ok(())
}

pub fn sample(ipaths: &[&Path], opath: &Path, rate: u32, seed: &[usize]) -> EGSResult<()> {
    assert!(ipaths.len() > 0, "Cannot combine zero files");
    let mut rng: StdRng = SeedableRng::from_seed(seed);
    let mut header = Header {
        mode: *b"MODE0",
        record_size: 28,
        using_zlast: false,
        total_particles: 0,
        total_photons: 0,
        min_energy: 1000.0,
        max_energy: 0.0,
        total_particles_in_source: 0.0,
    };
    let mut writer = PHSPWriter::from(File::create(opath)?, &header)?;
    for path in ipaths.iter() {
        let reader = PHSPReader::from(File::open(path)?)?;
        assert!(!reader.header.using_zlast);
        println!("Found {} particles", reader.header.total_particles);
        header.total_particles_in_source += reader.header.total_particles_in_source;
        let records = reader.filter(|_| rng.gen_weighted_bool(rate));
        for record in records.map(|r| r.unwrap()) {
            header.total_particles =
                header.total_particles.checked_add(1).expect("Total particles overflow");
            if !record.charged() {
                header.total_photons += 1;
            }
            if record.total_energy > 0.0 {
                header.min_energy = header.min_energy.min(record.total_energy);
                header.max_energy = header.max_energy.max(record.total_energy);
            }
            writer.write(&record)?;
        }
        println!("Now have {} particles", header.total_particles);
    }
    header.total_particles_in_source /= rate as f32;
    drop(writer);
    // write out the header
    let ofile = OpenOptions::new().write(true).create(true).open(opath)?;
    PHSPWriter::from(ofile, &header)?;
    Ok(())
}

pub fn transform(input_path: &Path, output_path: &Path, matrix: &[[f32; 3]; 3]) -> EGSResult<()> {
    let ifile = File::open(input_path)?;
    let reader = PHSPReader::from(ifile)?;
    let ofile;
    if input_path == output_path {
        println!("Transforming {} in place", input_path.display());
        ofile = OpenOptions::new().write(true).create(true).open(output_path)?;
    } else {
        // different path (create/truncate destination)
        println!("Transforming {} and saving to {}",
                 input_path.display(),
                 output_path.display());
        ofile = File::create(output_path)?;
    }
    let mut writer = PHSPWriter::from(ofile, &reader.header)?;
    let n_particles = reader.header.total_particles;
    let mut records_transformed = 0;
    for mut record in reader.map(|r| r.unwrap()) {
        record.transform(&matrix);
        writer.write(&record)?;
        records_transformed += 1;
    }
    println!("Transformed {} records, expected {}",
             records_transformed,
             n_particles);
    Ok(())
}
