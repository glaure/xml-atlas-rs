# Hilfe zu XML Atlas

XML Atlas öffnet große XML-Dateien schnell und zeigt jede als Baum, als Tabelle und als
Quelltext. Alle drei zeigen dasselbe Dokument, und eine Änderung in einer Ansicht erscheint
auch in den anderen.

## Dateien öffnen

- Datei > Neu (Strg+N) beginnt ein unbenanntes Dokument; beim Speichern wird nach seinem
  Namen gefragt.
- Datei > Öffnen (Strg+O), Dateien auf das Fenster ziehen oder sie auf der Befehlszeile
  nennen. Unter macOS auch per Doppelklick im Finder oder durch Ziehen auf das Symbol im Dock.
- Jede Datei öffnet sich in einem eigenen Tab. Strg+Tab und Strg+Umschalt+Tab wechseln den
  Tab, Strg+W schließt einen. Datei > Zuletzt geöffnet listet die letzten Dateien.
- Dateien in anderen Kodierungen als UTF-8 (Latin-1, UTF-16 und andere) öffnen sich, wie sie
  sind, und werden in ihrer eigenen Kodierung gespeichert.
- Auch eine Datei, die nicht wohlgeformt ist, öffnet sich. Die Liste der Probleme unter dem
  Text nennt jeden Fehler; ein Klick führt dorthin.
- Ändert ein anderes Programm die geöffnete Datei, bietet ein Hinweis über den Ansichten an,
  sie neu zu laden.
- Eine minifizierte Datei, deren Elemente in einer langen Zeile stehen, öffnet sich mit
  Zeilenumbruch und mit einem Hinweis, der anbietet, sie zu formatieren.

## Die Menüs

- Datei: Neu, Öffnen, Speichern, Schließen, Einstellungen und Beenden.
- Bearbeiten: Rückgängig und Wiederholen, die Zwischenablage, Suchen und Ersetzen sowie
  Element mit Umbenennen, Löschen, Duplizieren, Nach oben und Nach unten.
- Ansicht: die Anordnung, welche Bereiche zu sehen sind, die Optionen des Baums,
  Zeilenumbruch, die Symbolleiste, die Schrift und das Farbschema.
- Navigieren: Ebene höher und Ebene tiefer, zum Ziel eines Verweises oder zu einer Zeile
  gehen, den Tab wechseln und Lesezeichen.
- Extras: XPath, Formatieren und Kompakt sowie Verweise.
- Ein Eintrag, der gerade nicht anwendbar ist, ist ausgegraut. Ruht der Mauszeiger darauf,
  steht dort, warum.

## Die Symbolleiste

Unter dem Menü: Neu, Öffnen, Speichern, Ausschneiden, Kopieren, Einfügen, Rückgängig,
Wiederholen, Suchen, XPath, Ebene höher, Ebene tiefer, Formatieren und Diagramm. Ruht der
Mauszeiger auf einer Schaltfläche, erscheint ihr Name. Ansicht > Symbolleiste blendet sie aus.

## Der Baum

- Zeigt die Elemente, mit Attributen und Text als eigenen Zeilen (Ansicht > Attribute im Baum
  blendet die Zeilen der Attribute aus).
- Ein Klick wählt ein Element aus: Die Tabelle zeigt seine Kindelemente, der Text hebt es
  hervor.
- Die Pfeiltasten bewegen die Auswahl und klappen auf; Pos1, Ende, Bild auf und Bild ab
  springen. Alt+Pfeil hoch geht eine Ebene höher zum Elternelement, Alt+Pfeil runter eine
  Ebene tiefer in die Auswahl.
- Ein Rechtsklick auf ein Element bietet Umbenennen (F2), Löschen (Entf), Duplizieren
  (Strg+D) sowie Nach oben und Nach unten (Alt+Umschalt+Pfeil hoch und
  Alt+Umschalt+Pfeil runter). Bearbeiten > Element hat dieselben Befehle.

## Die Tabelle

- Zeigt die Kindelemente des ausgewählten Elements als Zeilen. Ihre Attribute (`@name`) und
  Kindelemente sind die Spalten. Eine Zelle, die für Kindelemente steht, zeigt deren Namen
  und Anzahl.
- Eine leere Zelle ist ein leerer Wert; bei einer schraffierten Zelle gibt es das Attribut
  oder Kindelement nicht.
- Ein Klick auf eine Überschrift sortiert nach dieser Spalte, ein weiterer kehrt die
  Reihenfolge um; `#` stellt die Reihenfolge im Dokument wieder her. Ein Rechtsklick auf eine
  Überschrift blendet Spalten aus.
- Ein Doppelklick auf eine Zeile oder die Eingabetaste öffnet sie. Die Rücktaste geht zurück
  zum Elternelement.
- Ein Klick auf eine Zelle und F2 bearbeiten ihren Wert; ein Wert mit mehr als 100.000
  Zeichen wird stattdessen im Text bearbeitet. Der Zellbereich neben der Tabelle zeigt die
  angeklickte Zelle ganz, bis zu 100.000 Zeichen; Ganzen Wert kopieren kopiert einen
  längeren (im Menü Ansicht lässt sich der Bereich abschalten).

## Das Diagramm

- Ansicht > Diagramm (Strg+Umschalt+D) zeigt anstelle der Tabelle den Aufbau des Dokuments:
  einen Kasten je Pfad aus Elementnamen, wie `orders/order/items/item`, mit der Anzahl der
  Elemente, die ihn haben, den Namen ihrer Attribute und `Text`, wenn sie Text enthalten.
  Gelesen wird der Aufbau, wenn das Diagramm zum ersten Mal gezeigt wird, und kurz nach einer
  Änderung erneut.
- Ein Klick auf einen Kasten wählt das erste Element mit diesem Pfad aus; ein weiterer Klick
  oder F3 das nächste. Der Kasten des Elements, das in Baum, Tabelle oder Text ausgewählt
  ist, ist markiert.
- Der kleine Kreis rechts an einem Kasten klappt seine Kinder zu und wieder auf. Jeder Tab
  merkt sich, was zugeklappt ist.
- Ziehen oder das Mausrad verschiebt den Ausschnitt; Strg+Mausrad vergrößert und
  verkleinert, Strg+0 stellt wieder 100 % ein.
- Ein Dokument, das ein Element in sich selbst verschachtelt (Abschnitte in Abschnitten),
  hat einen Pfad je Tiefe; das Diagramm zeigt die ersten 2.000 Pfade und sagt, wenn es mehr
  gibt.

## Der Text

- Der Quelltext, farbig hervorgehoben. Tippen Sie darin wie in jedem Editor; Baum und Tabelle
  folgen.
- Ein Klick auf eine Zeile wählt ihr Element im Baum aus.
- Alt+Z schaltet den Zeilenumbruch ein und aus. Sehr lange Zeilen lassen sich in beiden
  Fällen bis zum Ende lesen.
- Ansicht > Schrift… wählt Schriftart, Stil und Größe von Text, Baum und Tabelle.
  Aufgeführt sind nur Schriften mit fester Zeichenbreite.

## Verweise

- Für Dateien mit eigenen Verweisen, beschrieben in Regeldateien. Aus, bis Sie
  Extras > Verweise > Verweise auflösen einschalten.
- Datei > Einstellungen > Verweise listet die Regeldateien. Importieren Sie eine oder legen
  Sie eine neue an, schalten Sie sie ein und nennen Sie unter Verwenden für die Dokumente,
  für die sie gilt: `catalog*.xml` nach dem Namen, `*/XML/*catalog.xml` nach dem Pfad, `*`
  für jedes Dokument. Gelten mehrere für ein Dokument, wird die obere verwendet.
- Eine Regeldatei, die neben einer XML-Datei liegt, wird nicht verwendet: Importieren Sie
  sie.
- Bearbeiten (in dieser Liste) oder Regeln bearbeiten (in der Vorschau) öffnet eine
  Regeldatei mit einer Prüfung darunter: Fehler mit ihrer Zeile und wie oft jede Regel im
  angezeigten Dokument zutrifft. Einfügen setzt eine Vorlage an die Schreibmarke; Strg+S im
  Text speichert die Regeln, nicht das Dokument. Die Kopfzeile der Vorschau zeigt das
  Ergebnis der Prüfung; ein Klick darauf öffnet die Regeln.
- Ein Beispiel: `{"includes": [{"element": "Include", "attribute": "src"}], "contexts":
  {"Shared": {"path": "/Doc/Shared"}}}` lässt `<Include src="Shared:Parts"/>` für die
  `Parts` in `/Doc/Shared` stehen. `includes` nennt die Elemente, die Verweise sind, und das
  Attribut, das sagt, wohin; `contexts` gibt den Namen vor dem Doppelpunkt einen Ort.
- F12 geht dann zu dem, was der ausgewählte Verweis findet, und Verweise, die nichts finden,
  werden als Probleme aufgeführt.
- Extras > Verweise > Vorschau mit aufgelösten Verweisen zeigt das ausgewählte Element
  aufgelöst; Baum der Vorschau zeigt das als Baum. Die Datei wird nie geändert.

## Suchen und Ersetzen

- Strg+F sucht, Strg+H sucht und ersetzt. Die Eingabetaste oder F3 geht zum nächsten Treffer,
  Umschalt+Eingabe oder Umschalt+F3 zum vorigen. Groß-/Kleinschreibung und Ganzes Wort
  stehen in der Suchleiste.
- Ersetzen fügt den Ersatz so ein, wie er eingegeben ist, auch in Tags und Namen. Macht das
  das Dokument kaputt, sagt es die Statusleiste, und die Liste der Probleme zeigt, wo;
  Strg+Z nimmt es zurück. Alle ersetzen ist ein einziger Rückgängig-Schritt.

## XPath

- Extras > XPath (Strg+Umschalt+X) öffnet eine Abfrageleiste für einen XPath-1.0-Ausdruck,
  zum Beispiel `//order[@status='open']`, `count(//item)` oder
  `//order[@id='42']/customer/name`. Die Eingabetaste führt ihn aus; die Schaltfläche mit der
  Liste enthält die zuvor ausgeführten Ausdrücke. Im leeren Feld übernimmt Tab das gezeigte
  Beispiel. Während der Eingabe sagt die Leiste, ob der Ausdruck gültig ist. Ein falscher ist
  rot umrandet, die fehlerhafte Stelle ist unterstrichen, und die Meldung sagt, was dort
  erwartet wird.
- Die ausgewählten Knoten werden unten mit Zeile, Pfad und Wert aufgeführt. Ein Klick führt
  zu einem; F3 und Umschalt+F3 gehen sie durch, solange die Leiste offen ist. Eine Zahl, ein
  Text oder true/false wird in der Leiste angezeigt.
- Eine lange Abfrage zeigt eine laufende Anzeige; Anhalten bricht sie ab. Nach einer Änderung
  führt die Eingabetaste die Abfrage erneut aus.
- Namen ohne Präfix passen auch auf Elemente im Standardnamensraum des Wurzelelements. Text
  zwischen Elementen, der nur aus Leerraum besteht, ist kein Knoten.

## Formatieren

- Extras > Formatieren (Strg+I) rückt das Dokument ein; geändert wird nur der Leerraum
  zwischen Tags. Text, gemischter Inhalt und `xml:space="preserve"` bleiben, wie sie sind.
- Extras > Kompakt setzt das Dokument in eine Zeile.
- Extras > Formatierungsoptionen legt Einrückung, Tabulatoren und Attribute pro Zeile fest
  und ob Kommentare entfernt und Texte von Leerraum befreit werden.
- Jeder dieser Befehle ist ein einziger Rückgängig-Schritt.

## Rückgängig und Speichern

- Strg+Z macht rückgängig, Strg+Y oder Strg+Umschalt+Z wiederholt, auch nach dem
  Formatieren.
- Strg+S speichert, Strg+Umschalt+S speichert unter einem anderen Namen. Gespeichert wird
  atomar: Schlägt es fehl, ist die alte Datei noch ganz. Geändert wird nur, was Sie geändert
  haben; die Bytereihenfolge-Markierung (BOM) und die Zeilenenden bleiben.
- Wird ein Tab oder das Fenster mit nicht gespeicherten Änderungen geschlossen, wird vorher
  nachgefragt.

## Tastatur

- Strg+N neu, Strg+O öffnen, Strg+S speichern, Strg+Umschalt+S speichern unter, Strg+W Tab
  schließen, Strg+Q beenden
- Strg+Tab und Strg+Umschalt+Tab nächster und voriger Tab
- Strg+Z rückgängig, Strg+Y oder Strg+Umschalt+Z wiederholen, Strg+I formatieren
- Strg+F suchen, Strg+H ersetzen, F3 und Umschalt+F3 nächster und voriger Treffer
- Strg+Umschalt+X XPath; bei offener Leiste gehen F3 und Umschalt+F3 durch die Ergebnisse
- Strg+Umschalt+D Diagramm; nach einem Klick auf einen Kasten geht F3 zu seinem nächsten
  Element
- Alt+Pfeil hoch und Alt+Pfeil runter eine Ebene höher und tiefer; Alt+Umschalt+Pfeil hoch
  und Alt+Umschalt+Pfeil runter ein Element verschieben
- F2 umbenennen (Baum) oder eine Zelle bearbeiten (Tabelle); Entf löschen; Strg+D
  duplizieren
- F12 zum Ziel des Verweises; Alt+Z Zeilenumbruch; F1 diese Hilfe

Unter macOS tritt Cmd an die Stelle von Strg.

## KI-Agenten

Ein Agent wie Claude Code kann über XML Atlas mit XML-Dateien arbeiten, mit dem Model
Context Protocol (MCP):

- Mit Dateien, ohne Fenster: `claude mcp add xml-atlas -- xml-atlas-cli mcp`.
- In diesem Fenster, mit den offenen Tabs: Schalten Sie in Datei > Einstellungen
  KI-Agenten zulassen (MCP) ein, dann
  `claude mcp add xml-atlas-window -- xml-atlas-cli mcp --window`. Jede Änderung eines
  Agenten wird ausgewählt und in der Statusleiste genannt, und Strg+Z macht sie rückgängig.
  Einen Tab mit nicht gespeicherten Änderungen kann ein Agent nicht schließen.

Agenten lesen Elemente, Tabellen von Kindelementen, Textzeilen, Suchtreffer, Probleme und
XPath-Ergebnisse; sie ändern Texte, Attribute und Namen, löschen, duplizieren, verschieben,
ersetzen, formatieren, machen rückgängig und speichern.

## Einstellungen

Datei > Einstellungen… (Strg+,) zeigt alle Einstellungen in einem Fenster: Darstellung,
Lokalisierung, Ansichten, Verweise, Formatieren, Updates und KI-Agenten. Eine
Änderung gilt sofort. Unter Darstellung ist zu jedem Farbschema ein Beispiel zu sehen; ein
Klick darauf verwendet es.

Language / Sprache unter Lokalisierung wählt English (Voreinstellung), Deutsch oder die
Sprache des Systems.

## Updates

Einmal am Tag fragt XML Atlas GitHub nach seiner neuesten Version. Ist sie neuer, sagt es die
Statusleiste einmal, mit einem Link zur Download-Seite; ein Flatpak wird stattdessen mit der
Softwareverwaltung oder mit `flatpak update` aktualisiert. Die Anfrage enthält die Version
des Programms und sonst nichts, und nichts wird heruntergeladen oder installiert, solange Sie
nicht klicken. Ohne Netzwerk wird nichts angezeigt. Hilfe > Jetzt nach Updates suchen fragt
sofort und sagt auch, wenn Sie die neueste Version haben. Die tägliche Suche lässt sich in
Datei > Einstellungen > Updates abschalten.

Wurde XML Atlas aus dem `.deb`, dem `.rpm`, dem MSI für den aktuellen Benutzer oder dem DMG
installiert, bietet der Hinweis auch Herunterladen und installieren… an. Das speichert dieses
Paket in Ihrem Download-Ordner, sobald seine SHA-256-Prüfsumme die ist, die GitHub nennt, und
öffnet es mit dem Installationsprogramm des Systems, das nach den nötigen Rechten fragt.
Starten Sie XML Atlas unter Linux neu, wenn die Installation fertig ist. Unter Windows und
macOS klicken Sie zuerst auf Schließen und installieren…: XML Atlas fragt nach nicht
gespeicherten Änderungen, schließt sich und öffnet das Installationsprogramm (unter macOS
ziehen Sie die App auf Programme). Beim ZIP und bei einem für alle Benutzer installierten MSI
gibt es nur den Link. Wo kein Installationsprogramm das Paket öffnet, wird stattdessen die
Download-Seite geöffnet.

Anordnung, Farbschema, Ansichtsoptionen, Schrift des Texts, Formatierungsoptionen und zuletzt
geöffnete Dateien werden in `xml-atlas/settings.toml` im Konfigurationsordner des Systems
gespeichert. Ebenso die Größen des Fensters und der Bereiche; Ansicht > Anordnung >
Anordnung zurücksetzen gibt den Bereichen wieder ihre anfänglichen Größen.
