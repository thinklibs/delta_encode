use super::*;

impl DeltaEncodable for cgmath::Vector3<f32> {
    #[inline]
    fn encode<W>(&self, _base: Option<&Self>, w: &mut Writer<W>) -> io::Result<()>
        where W: Write
    {
        w.write_f32(self.x)?;
        w.write_f32(self.y)?;
        w.write_f32(self.z)?;
        Ok(())
    }

    #[inline]
    fn decode<R>(_base: Option<&Self>, r: &mut Reader<R>) -> io::Result<Self>
        where R: Read
    {
        Ok(cgmath::Vector3::new(
            r.read_f32()?,
            r.read_f32()?,
            r.read_f32()?,
        ))
    }
}