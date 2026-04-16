pub type Integer = i64;
pub type Float = f32;

#[derive(Clone, Debug, defmt::Format)]
pub enum Value {
    Null,
    Bool(bool),
    Integer(Integer),
    Float(Float),
}
