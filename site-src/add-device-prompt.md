I want to add one of my devices to the Hardpoint catalog: {repo}

Please do this with me, one step at a time.

1. Clone that repository and read README.md, docs/SPEC.md and catalog/vocabulary/capabilities.toml.
2. Run `python3 tools/observe.py`. Show me the devices it lists that have no card yet, and ask me which one to add.
3. Run `python3 tools/observe.py --draft <number>` and save the draft at the path it names.
4. Fill in the card:
   - Use only capability ids from the vocabulary. If one is truly missing, propose adding it and ask me first.
   - Use only adapters listed in catalog/adapters/adapters.toml.
   - Never give a capability a lower risk than the vocabulary's min_risk.
   - Never put a command, a script or a file path in a card.
5. Be exact about what has been checked:
   - Use "observed", with today's date, only for something you ran on this computer and saw work. Say what you ran in the note.
   - Use "unverified" for anything you only read about, and say where.
   - Do not use "tested".
   - Only read from my device. Do not change any setting on it to test something unless I say yes first.
6. Write from what you observe and from the maker's documentation. Do not copy data files from other projects. List your sources.
7. Run `python3 tools/build.py --check` and fix what it reports until it passes.
8. Show me the finished card, then open a pull request with it.
