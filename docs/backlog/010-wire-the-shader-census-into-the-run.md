# Wire the shader census into the run report *(half done)*


The CLI half is done: `orbistoun-cli shaders <dir>` ranks what blocks translation across a
corpus, as a thin shim over `orbistoun-shader::report`.

The run-report half is not. The census numbers do not go through the
diff-against-previous machinery, so "did that change unblock anything?" is answered by
reading two outputs side by side rather than by a verdict - which is exactly the thing the
import side stopped doing at D047.

