pub(crate) fn take_next_line(buffer: &mut String) -> Option<String> {
    let newline_idx = buffer.find('\n')?;
    let mut line = buffer[..newline_idx].to_owned();
    buffer.drain(..=newline_idx);
    trim_cr(&mut line);
    Some(line)
}

pub(crate) fn trim_cr(line: &mut String) {
    if line.ends_with('\r') {
        line.pop();
    }
}
