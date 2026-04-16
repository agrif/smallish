pub type Integer = i64;
pub type Float = f32;

#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Value {
    Null,
    Bool(bool),
    Integer(Integer),
    Float(Float),
}
