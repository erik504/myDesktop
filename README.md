# myDesktop

Utility per Windows 10 e 11 che memorizza e ripristina la disposizione delle finestre sui monitor. Avviare `mydesktop.exe`: l'icona appare nell'area di notifica, eventualmente nel menu delle icone nascoste. Fare clic destro sull'icona e scegliere **Memorizza finestre**, **Ripristina finestre** oppure **Esci**.

La memorizzazione sostituisce la disposizione salvata in precedenza nel file `%APPDATA%\myDesktop\layout.json`, che può contenere percorsi e titoli di finestre personali. Il ripristino non chiude altre applicazioni. Le finestre vengono abbinate ai monitor salvati e, se un monitor non è disponibile, collocate sul monitor principale. Per Esplora file viene salvata la cartella della finestra; se non è leggibile, il salvataggio segnala l'errore e conserva la configurazione precedente.

## Compilazione

Installare la toolchain Rust per Windows, quindi eseguire:

```powershell
cargo build --release
```

Il programma compilato si trova in `target\release\mydesktop.exe`. L'eseguibile non richiede una finestra di terminale e può essere spostato in una cartella a scelta.

## Limiti

Windows non espone un sistema universale per salvare e riaprire esattamente lo stato interno di ogni applicazione. Alcuni programmi usano più processi o gestiscono le finestre autonomamente; altri richiedono privilegi elevati, un accesso, documenti o argomenti particolari. Il risultato dipende da questi programmi e non è garantito per tutte le applicazioni. Le finestre elevate potrebbero non essere modificabili da un programma eseguito senza gli stessi privilegi. Le schede multiple nella stessa finestra di Esplora file non vengono ricostruite individualmente. Se un monitor manca o la sua risoluzione è cambiata, la disposizione può richiedere adattamenti.
