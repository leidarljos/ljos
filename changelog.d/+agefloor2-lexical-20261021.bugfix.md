A memory that names the prompt's subject reaches the prompt on a
pack that sends no cosine, however old it is. That fallback held
the memory to 0.6 of the top fused score, and the pack decays that
score by age, so a correct old memory lost its place to a fresh
one. The floor is now 0.6 of the best lexical match, which carries
no age. What gets in is ordered by how much of the prompt it
covers, or by the cross-encoder when it ran, and not by the fused
score.
