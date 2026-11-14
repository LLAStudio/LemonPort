#!/usr/bin/env python3
"""Reject non-English string literals in LemonPort sources.

The project is English-only. Line comments are ignored; only Rust string
literals are checked. Accented French characters and a curated list of
distinctive French words are reported.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ACCENTED_FRENCH = re.compile(r"[éèàçùôîêâëïûœÉÈÀÇÙÔÎÊÂËÏÛŒ]")
FRENCH_WORDS = {
    "aucun",
    "aucune",
    "processus",
    "projet",
    "projets",
    "fichier",
    "fichiers",
    "dossier",
    "dossiers",
    "utilisateur",
    "utilisateurs",
    "doit",
    "doivent",
    "peut",
    "peuvent",
    "lancement",
    "attente",
    "arret",
    "libere",
    "liberation",
    "masque",
    "verifie",
    "affiche",
    "garde",
    "respecte",
    "restaure",
    "annule",
    "quitte",
    "refuse",
    "echec",
    "delai",
    "presse",
    "papier",
    "navigateur",
    "ouverture",
    "rafraichi",
    "rafraichis",
    "recherche",
    "selectionne",
    "ecoute",
    "demarrage",
    "compteur",
    "systeme",
    "protege",
    "ecran",
    "deja",
    "etre",
    "premiere",
    "derniere",
    "donc",
    "alors",
    "encore",
    "jamais",
    "toujours",
    "seulement",
    "vraiment",
    "tres",
    "trop",
    "assez",
    "environ",
    "chemin",
    "tuer",
    "ouvrir",
    "copier",
    "fermer",
    "quitter",
    "filtrer",
    "rafraichir",
    "lancer",
    "arreter",
    "liberer",
    "filtre",
    "ligne",
    "copie",
    "ouvre",
}

WORD = re.compile(r"[A-Za-zÀ-ÿ]+")


def string_literals(line: str) -> list[str]:
    """Extract normal Rust string literals from one source line."""
    literals: list[str] = []
    index = 0
    while index < len(line):
        if line.startswith("//", index):
            break
        if line[index] != '"':
            index += 1
            continue

        index += 1
        value: list[str] = []
        while index < len(line):
            character = line[index]
            if character == "\\" and index + 1 < len(line):
                value.append(line[index : index + 2])
                index += 2
                continue
            if character == '"':
                literals.append("".join(value))
                index += 1
                break
            value.append(character)
            index += 1
    return literals


def french_findings(value: str) -> list[str]:
    findings: list[str] = []
    if ACCENTED_FRENCH.search(value):
        findings.append("accented French text")
    words = {word.lower() for word in WORD.findall(value)}
    matches = sorted(words & FRENCH_WORDS)
    if matches:
        findings.append("French words: " + ", ".join(matches))
    return findings


def check_file(path: Path) -> list[tuple[int, str, list[str]]]:
    problems: list[tuple[int, str, list[str]]] = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        for literal in string_literals(line):
            findings = french_findings(literal)
            if findings:
                problems.append((number, literal, findings))
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "paths",
        nargs="*",
        type=Path,
        help="files or directories to check (default: src)",
    )
    args = parser.parse_args()

    root = Path(__file__).resolve().parent.parent
    paths = args.paths or [root / "src"]
    files: list[Path] = []
    for path in paths:
        if path.is_dir():
            files.extend(sorted(path.rglob("*.rs")))
        elif path.suffix == ".rs":
            files.append(path)

    failures = 0
    for path in files:
        for line, literal, findings in check_file(path):
            failures += 1
            print(f"{path}:{line}: {literal!r} ({'; '.join(findings)})")

    if failures:
        print(f"\n{failures} non-English UI string(s) found.", file=sys.stderr)
        return 1

    print(f"English UI check passed ({len(files)} Rust file(s) scanned).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
