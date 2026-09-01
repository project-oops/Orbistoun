# Ordered mounts for base-plus-patch

Per D054, a patch shadows base files and can carry its own executable. `orbistoun-fs`
needs ordered mounts rather than a single root, and title identity must hash whichever
executable wins the merge. Not urgent - the material to hand is add-only - but loading
a superseded executable would produce behaviour matching no real installation, which is
a hard symptom to diagnose.

