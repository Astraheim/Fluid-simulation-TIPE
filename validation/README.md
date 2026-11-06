# Contrôle fonctionnel du 6 novembre 2026

`smoke-results.csv` contient douze essais courts sur la grille N=510 : les six bateaux de `--init-campaign`, à U=0,6 puis U=1, avec 10 pas de stabilisation et 20 pas mesurés. Ils ont tous produit un statut `completed`, un CSV pas à pas, une synthèse et un score. Le tableau conserve les moyennes et écarts types ainsi que le score aux poids par défaut.

Il s'agit de **tests de fonctionnement**, pas d'une étude de convergence ni d'un classement physique validé. Après seulement 30 pas, l'écoulement n'a pas nécessairement atteint le bateau ou un régime représentatif. Ne pas utiliser ces scores pour choisir une géométrie.

Exécution effectuée avec les modules Rust du projet directement inclus dans un exécutable de validation sans fenêtre, compilé en mode release. Les durées incluent injection et pas de simulation, excluent rendu, calcul des statistiques et écriture. Les sources numériques sont identiques à celles de l'application ; l'empreinte de construction du binaire de validation est distincte.

Vérifications complémentaires : 12 tests Rust réussis (étanchéité, coque, supports sans vide, sérialisation/recharge, rejet des configurations invalides, calcul statistique, répétition du calcul, fichiers de résultats, exclusion des essais interrompus, sens du score et résidu nul au repos), et `cargo check --tests` réussi sur l'application complète.

Pour refaire cette vérification : générer `campaign.json`, remplacer `warmup` par 10 et `measure` par 20 dans chaque bateau, puis lancer `--batch`. Pour des résultats exploitables, augmenter les durées et examiner les diagnostics décrits dans [EXPERIMENTS.md](../EXPERIMENTS.md).
