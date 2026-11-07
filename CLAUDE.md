# Apsophos — istruzioni per Claude

Denoiser di foto in Rust + egui (SCUNet, ONNX Runtime con TensorRT for RTX,
DirectML e CPU). Prima di cominciare leggi `claude/status.md` (ultimo commit
controllato, lavoro in corso, punti aperti, decisioni) e `TIDY.md` (cose
rimandate). Ciò che vale solo su questo PC (percorsi locali) è in
`CLAUDE.local.md`, fuori dal repository.

## Regole di lavoro

- Lo sviluppo avviene in una sola chat, che scrive il codice, compila,
  controlla, corregge e unisce i rami.
- Si sviluppa in locale con git; il repository è su GitHub. L'agente
  pubblica solo il ramo di lavoro, con il comando delle regole generali, e
  la CI parte a ogni push di un ramo; push di `main`, tag e release li fa il
  proprietario.
- Un ramo di lavoro alla volta, partendo dall'ultimo `main` controllato.
- Dopo ogni blocco di modifiche lancia `scripts\check.bat`, leggi
  `target\check-summary.txt` e correggi finché è tutto verde; si unisce a
  `main` solo dopo `scripts\check.bat` tutto verde e la CI del ramo verde.
- Rispondi in italiano; interfaccia (lingua sorgente), codice, commenti,
  messaggi di commit, `claude/status.md`, `TIDY.md` e `docs/` in inglese.
- Alla fine di ogni blocco di lavoro aggiorna `TIDY.md` (cose rimandate) e
  `claude/status.md` (ultimo commit controllato, lavoro in corso, punti
  aperti, decisioni).

## Skill

Sempre, prima di scrivere o rivedere codice:

- `software-design-core`
- `rust-egui-desktop-design`
- `image-processing-design` (pipeline, colore, riquadri, ONNX Runtime)
- `ui-design-core` (ogni modifica che si vede o si sente nell'interfaccia)

Per argomento:

- lettura dei file e dei metadati (`denoise_io`), profili ICC, caricamento
  dei modelli, build, firma e rilascio: `security-design`
- scrittura dei file esportati e delle impostazioni: `durable-data-design`
- DLL, percorsi, processi e integrazione con Windows: `windows-system-design`
- un viewer o un calcolo su wgpu (oggi il viewer usa le texture di egui):
  `gpu-design`

## Eccezioni alle regole e alle skill

- **Interfaccia in grigi, senza rosso né ambra per errori e avvisi**
  (regola generale sul colore, skill `ui-design-core`): l'unico colore sullo
  schermo dev'essere la foto, che si giudica nei colori (regola del
  contenuto prima della cornice, skill `image-processing-design`). Errori e
  avvisi si distinguono con icona e testo; l'accento è un grigio chiaro.
- **Catalogo dei testi in Rust (`text.rs`) invece di Fluent** (skill
  `rust-egui-desktop-design`): ogni lingua è un valore di una struttura, così
  il compilatore controlla che ogni lingua abbia ogni testo e i plurali sono
  funzioni; non ci sono file da leggere né errori di risoluzione a runtime.
  Si torna a Fluent se arrivano traduttori esterni o più di tre lingue.
- **Percorsi locali tolti dagli eseguibili con una variabile d'ambiente**
  (regola sulle opzioni del compilatore solo in file versionati):
  `scripts\common.ps1` imposta `--remap-path-prefix` per il target Windows,
  perché i prefissi sono percorsi di questo PC e `trim-paths` di Cargo non è
  ancora stabile (Rust 1.99). Gli altri flag (CFG, CET) sono in
  `.cargo/config.toml`. Si passa a `trim-paths` quando è stabile.
- **`unsafe` senza Miri in `denoise_platform`** (regola su `unsafe`): il
  crate fa solo due chiamate di Windows (ricerca delle DLL, rinomina
  durevole) e Miri non esegue chiamate esterne; ogni blocco ha il suo
  `SAFETY:` e i test eseguono le chiamate vere. Il resto del workspace ha
  `unsafe_code = "forbid"`.

## Struttura

| Cosa | Dove |
|---|---|
| Crate | `crates/` (`denoise_core`, `denoise_io`, `denoise_engine`, `denoise_platform`, app egui) |
| Script | `scripts\prepare.bat`, `check.bat`, `run.bat`, `self-test.bat`, `package.bat` |
| Modelli | `models\` (FP32 e FP16, riquadri 448–1600), non nel repository |
| Threat model | `docs/threat-model.md` (file aperti, modelli, rilascio) |

Solo Windows per le prove; Rust 1.99.1 (`rust-toolchain.toml`).
