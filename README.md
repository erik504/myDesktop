# myDesktop

Utility per Windows 10 e 11 che memorizza e ripristina la disposizione delle finestre sui monitor. Avviare `mydesktop.exe`: l'icona appare nell'area di notifica, eventualmente nel menu delle icone nascoste. Fare clic destro sull'icona e scegliere **Memorizza finestre**, **Ripristina finestre** oppure **Esci**.

Il menu propone tre modalità per la configurazione predefinita:

- **Ripristina finestre** riposiziona le finestre riconosciute e prova a riaprire quelle mancanti, lasciando intatte le app aggiuntive.
- **Ripristina finestre (solo presenti)** riposiziona solo le finestre già aperte e riconoscibili; non avvia app e non apre finestre.
- **Ripristina finestre (chiudi estranee)** esegue il ripristino normale e, dopo conferma, richiede la chiusura delle app visibili assenti dal salvataggio. Le finestre aggiuntive di app presenti nel salvataggio rimangono aperte. Se il ripristino non è completo, non viene chiusa alcuna app aggiuntiva.

La modalità «chiudi estranee» invia alle finestre la normale richiesta di chiusura: le app possono chiedere di salvare il lavoro. Non termina forzatamente i processi; le app con privilegi maggiori potrebbero rifiutare la richiesta e quelle senza finestre visibili non sono gestite. Vengono considerate solo le app presenti quando inizia il ripristino, evitando di chiudere app aperte successivamente. Lasciando il puntatore su una voce del menu compare una breve spiegazione.

La voce **Avvia con Windows** installa il servizio e abilita l'avvio del tray al login dell'utente corrente. È richiesta la conferma amministrativa UAC dello stesso utente. Disattivandola, il servizio viene fermato e disinstallato e l'avvio automatico disabilitato; il tray rimane aperto fino a quando si sceglie **Esci**. Il singolo eseguibile rimane copiato in `%ProgramFiles%\myDesktop` in una cartella protetta per consentire una successiva riattivazione. Durante le operazioni il tray mostra uno spinner; una spunta verde indica il successo. Gli errori vengono mostrati in una finestra di dialogo.

I comandi **Memorizza finestre come...** e **Ripristina finestre da...** permettono di scegliere file di configurazione con nome. I comandi senza il suffisso usano sempre il file predefinito descritto sotto.

**Memorizza una finestra...** permette di aggiornare o aggiungere una singola finestra nella configurazione predefinita, lasciando invariate tutte le altre. Muovi il mouse sulla finestra evidenziata e fai clic per confermare; premi **Esc** o fai clic destro per annullare. Il clic di conferma non viene inviato all'app selezionata. Per Esplora file viene memorizzata anche la cartella corrente. Se non è possibile identificare con certezza una voce già salvata, il file rimane invariato e l'errore viene segnalato nell'icona del tray.

Il servizio permette di riposizionare finestre elevate dello stesso utente e della stessa sessione. L'interfaccia continua ad aprire i programmi con i normali permessi dell'utente; la riapertura di applicazioni che richiedono privilegi elevati può richiedere UAC. Il servizio non esegue i programmi specificati nelle configurazioni. I processi dello stesso utente e della stessa sessione autorizzata possono richiedere al servizio di spostare le finestre di quell'utente. L'installazione richiede che l'utente corrente sia amministratore: le credenziali di un altro account non sono supportate.

La memorizzazione sostituisce la disposizione salvata in precedenza nel file `%APPDATA%\myDesktop\layout.json`, che può contenere percorsi e titoli di finestre personali. Il ripristino normale e quello «solo presenti» non chiudono altre applicazioni. Le finestre vengono abbinate ai monitor salvati e, se un monitor non è disponibile, collocate sul monitor principale. Per Esplora file viene salvata la cartella della finestra; se non è leggibile, il salvataggio segnala l'errore e conserva la configurazione precedente.

Gli errori sono segnalati nell'icona del tray: per leggerli, porta il mouse sull'icona. Il segnale scompare dopo la visualizzazione. Le finestre già nella posizione e nello stato salvati non vengono modificate; quelle visibili già aperte vengono spostate, quando possibile, con un'animazione di circa 250 ms. Le app mancanti vengono avviate in parallelo: Windows riceve la posizione iniziale desiderata e, se un'app non la rispetta, la finestra riconosciuta viene spostata senza animazione. Le finestre che richiedono il servizio privilegiato possono spostarsi senza animazione e le richieste a tale servizio vengono gestite una alla volta.

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

Per applicazioni come Gestione attività e MSI Afterburner, Windows può richiedere la conferma UAC quando vengono riaperte. Il servizio deve essere installato e disponibile per spostare finestre elevate da myDesktop non elevato. MSI Afterburner gestisce autonomamente le finestre secondarie, come quella dei grafici: se l'applicazione non le ricrea al riavvio, myDesktop non può garantirne l'apertura. Gli eventuali errori vengono segnalati al termine del ripristino.

Per distinguere le finestre di ogni applicazione viene conservata, quando disponibile, l'identità della finestra e del processo; dopo un riavvio viene usato il titolo solo se identifica la finestra senza ambiguità. Per Esplora file viene considerata anche la cartella. Se non è possibile riconoscere con certezza una finestra, viene segnalata invece di spostare un'altra finestra. Le configurazioni precedenti restano leggibili; per salvare anche le nuove identità occorre memorizzarle nuovamente. Le schede di Chrome e Firefox e i documenti delle altre applicazioni non vengono salvati né ricostruiti direttamente da myDesktop.
