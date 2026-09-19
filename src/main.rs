use std::env;
use std::fs;
use std::process;

const BYTE_REGISTERS: [&str; 8] = ["al", "cl", "dl", "bl", "ah", "ch", "dh", "bh"];
const WORD_REGISTERS: [&str; 8] = ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"];

const MOV_REG_MEMORY_MASK: u8 = 0b1111_1100;
const MOV_REG_MEMORY_OPCODE: u8 = 0b1000_1000;
const MOV_IMMEDIATE_MASK: u8 = 0b1111_0000;
const MOV_IMMEDIATE_OPCODE: u8 = 0b1011_0000;

#[derive(Clone, Copy)]
enum Width {
    Byte,
    Word,
}

impl Width {
    fn from_bit(bit: u8) -> Self {
        if bit == 0 { Self::Byte } else { Self::Word }
    }

    fn register(self, code: u8) -> &'static str {
        match self {
            Self::Byte => BYTE_REGISTERS[code as usize],
            Self::Word => WORD_REGISTERS[code as usize],
        }
    }
}

#[derive(Clone, Copy)]
enum AddressingMode {
    NoDisplacement,
    ByteDisplacement,
    WordDisplacement,
    Register,
}

impl AddressingMode {
    fn from_bits(bits: u8) -> Self {
        match bits {
            0 => Self::NoDisplacement,
            1 => Self::ByteDisplacement,
            2 => Self::WordDisplacement,
            3 => Self::Register,
            _ => unreachable!("mod field is always two bits"),
        }
    }
}

pub struct X86Decoder {
    source: Vec<u8>,
    current: usize,
}

impl X86Decoder {
    pub fn new(source: Vec<u8>) -> Self {
        Self { source, current: 0 }
    }

    pub fn decode(&mut self) -> Result<Vec<String>, String> {
        let mut instructions = Vec::new();

        while self.current < self.source.len() {
            instructions.push(self.decode_instruction()?);
        }

        Ok(instructions)
    }

    fn decode_instruction(&mut self) -> Result<String, String> {
        let opcode = self.read_byte()?;

        if opcode & MOV_REG_MEMORY_MASK == MOV_REG_MEMORY_OPCODE {
            self.decode_register_memory_mov(opcode)
        } else if opcode & MOV_IMMEDIATE_MASK == MOV_IMMEDIATE_OPCODE {
            self.decode_immediate_mov(opcode)
        } else {
            Err(format!(
                "unsupported opcode {opcode:#04x} at byte {}",
                self.current - 1
            ))
        }
    }

    fn decode_register_memory_mov(&mut self, opcode: u8) -> Result<String, String> {
        let direction_is_to_register = opcode & 0b10 != 0;
        let width = Width::from_bit(opcode & 0b1);
        let mod_reg_rm = self.read_byte()?;
        let mode = AddressingMode::from_bits(mod_reg_rm >> 6);
        let register = width.register((mod_reg_rm >> 3) & 0b111);
        let rm = mod_reg_rm & 0b111;
        let rm_operand = self.decode_rm_operand(mode, rm, width)?;

        let (destination, source) = if direction_is_to_register {
            (register.to_owned(), rm_operand)
        } else {
            (rm_operand, register.to_owned())
        };

        Ok(format!("mov {destination}, {source}"))
    }

    fn decode_immediate_mov(&mut self, opcode: u8) -> Result<String, String> {
        let width = Width::from_bit((opcode >> 3) & 0b1);
        let register = width.register(opcode & 0b111);
        let immediate = match width {
            Width::Byte => i8::from_le_bytes([self.read_byte()?]).to_string(),
            Width::Word => self.read_i16()?.to_string(),
        };

        Ok(format!("mov {register}, {immediate}"))
    }

    fn decode_rm_operand(
        &mut self,
        mode: AddressingMode,
        rm: u8,
        width: Width,
    ) -> Result<String, String> {
        if matches!(mode, AddressingMode::Register) {
            return Ok(width.register(rm).to_owned());
        }

        if matches!(mode, AddressingMode::NoDisplacement) && rm == 6 {
            return Ok(format!("[{}]", self.read_u16()?));
        }

        let base = match rm {
            0 => "bx + si",
            1 => "bx + di",
            2 => "bp + si",
            3 => "bp + di",
            4 => "si",
            5 => "di",
            6 => "bp",
            7 => "bx",
            _ => unreachable!("r/m field is always three bits"),
        };
        let displacement: Option<i16> = match mode {
            AddressingMode::NoDisplacement => None,
            AddressingMode::ByteDisplacement => Some(i8::from_le_bytes([self.read_byte()?]) as i16),
            AddressingMode::WordDisplacement => Some(self.read_i16()?),
            AddressingMode::Register => unreachable!(),
        };

        Ok(match displacement {
            Some(displacement) => format!("[{base} + {displacement}]"),
            None => format!("[{base}]"),
        })
    }

    fn read_byte(&mut self) -> Result<u8, String> {
        let byte = self
            .source
            .get(self.current)
            .copied()
            .ok_or_else(|| format!("unexpected end of input at byte {}", self.current))?;
        self.current += 1;
        Ok(byte)
    }

    fn read_u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes([self.read_byte()?, self.read_byte()?]))
    }

    fn read_i16(&mut self) -> Result<i16, String> {
        Ok(i16::from_le_bytes([self.read_byte()?, self.read_byte()?]))
    }
}

fn main() {
    let filename = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: perfaware <8086 binary>");
        process::exit(2);
    });
    let source = fs::read(&filename).unwrap_or_else(|error| {
        eprintln!("could not read {filename}: {error}");
        process::exit(1);
    });

    let mut decoder = X86Decoder::new(source);
    let instructions = decoder.decode().unwrap_or_else(|error| {
        eprintln!("could not decode {filename}: {error}");
        process::exit(1);
    });

    println!("bits 16\n");
    for instruction in instructions {
        println!("{instruction}");
    }
}

#[cfg(test)]
mod tests {
    use super::X86Decoder;

    fn decode(bytes: &[u8]) -> Vec<String> {
        X86Decoder::new(bytes.to_vec()).decode().unwrap()
    }

    #[test]
    fn decodes_immediate_moves() {
        assert_eq!(
            decode(&[0xb0, 0xff, 0xbb, 0x34, 0x12]),
            ["mov al, -1", "mov bx, 4660"]
        );
    }

    #[test]
    fn decodes_register_and_memory_moves_in_both_directions() {
        assert_eq!(
            decode(&[0x8b, 0x00, 0x88, 0xd9, 0x89, 0x5e, 0xfe]),
            ["mov ax, [bx + si]", "mov cl, bl", "mov [bp + -2], bx"]
        );
    }

    #[test]
    fn decodes_direct_addresses_and_word_displacements() {
        assert_eq!(
            decode(&[0x8b, 0x1e, 0x34, 0x12, 0x89, 0x87, 0xfc, 0xff]),
            ["mov bx, [4660]", "mov [bx + -4], ax"]
        );
    }

    #[test]
    fn reports_truncated_instructions() {
        let error = X86Decoder::new(vec![0x8b]).decode().unwrap_err();

        assert_eq!(error, "unexpected end of input at byte 1");
    }
}
