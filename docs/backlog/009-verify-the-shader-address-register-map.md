# Verify the shader-address register map


`crates/orbistoun-gpu/data/packets.toml` claims which registers hold shader addresses.
Unlike the encoding table there is no reference to diff against, so the entries are a
hypothesis and the code reports `ShaderCandidate` rather than an address (D091).

The check needs a real submission: walk it, extract the candidates, and see whether the
addresses they yield point at something that decodes as a shader. That is a strong
self-check - a wrong register produces an address pointing at data, and data does not
decode cleanly - but it needs the emulator to reach a submission first.

