#let toto = (name: "TOTO", age: 5, ville:"Toulouse")

= Rapport trimestriel

Ce trimestre, les ventes ont augmenté de 12 pourcent par rapport
au trimestre précédent, grâce à la nouvelle campagne marketing.

Le principal client reste la société Martin.

#table(
    columns: (1fr,1fr,1fr),
    [#toto.name],[#toto.age],[#toto.ville]
)
