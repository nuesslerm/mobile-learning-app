//! Pure compute for the remote-chat-agent service: markdown to HTML, syntax highlighting, the Tamil marking picture and the text of
//! a Word file. No process, no network, no clock: a change to a route does not rebuild the grammars and the rasteriser here.

pub mod docx;
pub mod highlight;
pub mod js;
pub mod md;
pub mod tamil;

#[cfg(test)]
mod tests;
