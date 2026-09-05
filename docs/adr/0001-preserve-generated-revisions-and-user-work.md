# Preserve generated revisions and user work

Retrying processing can change speaker assignments, summaries, and extracted tasks after a user has reviewed them. Locus publishes replacements atomically, retains previous summary revisions and their completion state, and marks dependent results outdated; regeneration of an existing summary is explicit. This costs revision and provenance tracking but prevents a successful retry from silently destroying reviewed work.

