# myDesktop

Utility per Windows 10 e 11 che memorizza e ripristina la disposizione delle finestre sui monitor. Avviare `mydesktop.exe`: l'icona appare nell'area di notifica, eventualmente nel menu delle icone nascoste. Fare clic destro sull'icona e scegliere **Memorizza finestre**, **Ripristina finestre** oppure **Esci**.

La voce **Avvia con Windows** abilita o disabilita l'avvio al login dell'utente corrente. Durante le operazioni il tray mostra uno spinner; una spunta verde indica il successo. Gli errori vengono mostrati in una finestra di dialogo.

La memorizzazione sostituisce la disposizione salvata in precedenza nel file `%APPDATA%\myDesktop\layout.json`, che può contenere percorsi e titoli di finestre personali. Il ripristino non chiude altre applicazioni. Le finestre vengono abbinate ai monitor salvati e, se un monitor non è disponibile, collocate sul monitor principale. Per Esplora file viene salvata la cartella della finestra; se non è leggibile, il salvataggio segnala l'errore e conserva la configurazione precedente.

## Compilazione

Installare la toolchain Rust per Windows e il Windows SDK (incluso `rc.exe`), quindi eseguire:

```powershell
cargo build --release
```

Il programma compilato si trova in `target\release\mydesktop.exe`. L'eseguibile non richiede una finestra di terminale e può essere spostato in una cartella a scelta.

## Versione

La versione dell'app parte da `0.0.1` e cresce di un'unità per ogni commit sulla cronologia principale: dopo `0.0.99` viene `0.1.0`, e dopo `0.99.99` viene `1.0.0`. Ogni componente va da 0 a 99. È visibile dal menu del tray e nelle proprietà dell'eseguibile. Il numero è calcolato durante la compilazione a partire dalla storia Git: un eseguibile già compilato conserva la propria versione finché non viene ricompilato. I commit precedenti all'introduzione di questa numerazione non vengono contati. Per calcolare il numero serve un clone completo della cronologia; per compilare un archivio senza Git impostare esplicitamente la versione corretta, per esempio `$env:MYDESKTOP_BUILD_VERSION = '0.0.1'`, prima di eseguire Cargo.

## Limiti

Windows non espone un sistema universale per salvare e riaprire esattamente lo stato interno di ogni applicazione. Alcuni programmi usano più processi o gestiscono le finestre autonomamente; altri richiedono privilegi elevati, un accesso, documenti o argomenti particolari. Il risultato dipende da questi programmi e non è garantito per tutte le applicazioni. Le finestre elevate potrebbero non essere modificabili da un programma eseguito senza gli stessi privilegi. Le schede multiple nella stessa finestra di Esplora file non vengono ricostruite individualmente. Se un monitor manca o la sua risoluzione è cambiata, la disposizione può richiedere adattamenti.

Per applicazioni come Gestione attività e MSI Afterburner, Windows può richiedere la conferma UAC quando vengono riaperte. Per intervenire su finestre di un'applicazione eseguita come amministratore può essere necessario avviare anche myDesktop come amministratore. MSI Afterburner gestisce autonomamente le finestre secondarie, come quella dei grafici: se l'applicazione non le ricrea al riavvio, myDesktop non può garantirne l'apertura. Gli eventuali errori vengono segnalati al termine del ripristino.
