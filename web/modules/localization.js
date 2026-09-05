(() => {
    const DEFAULT_LANGUAGE = 'en';
    const SUPPORTED_LANGUAGES = Object.freeze([
        'en',
        'it',
        'pl',
        'es',
        'fr',
        'de',
        'pt-br',
        'ja'
    ]);
    const STORAGE_KEY = 'deltamodCommunityLanguage';
    const dictionaries = new Map();
    const metadata = new Map();

    const communityStrings = Object.freeze({
        en: {
            refine_load_failed: "Could not load the mod list.",
            refine_retry: "Retry",
            refine_search_mods: "Search mods",
            refine_search_hint: "Name, author, version or package ID",
            refine_clear: "Clear",
            refine_mod_count: "{0} of {1} mods",
            refine_no_matches: "No matching mods. Clear the search or change the filter.",
            refine_saving: "Saving…",
            refine_saved: "Saved",
            refine_save_failed: "Not saved. Try again.",
            refine_progress: "Progress",
            refine_patch_complete: "Patching complete!",
            refine_patch_log: "Patch log",

            community_options_subtitle: 'Configure Community without changing the official Deltamod profile.',
            optcat_data: 'Data',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Choose the language used by Deltamod Community. New Community features may fall back to English until their translations are updated.',
            language_current: 'Current language',
            language_select_hint: 'Select language',
            language_restart_note: 'The current page refreshes immediately after changing language.',
            theme_count: '{0} of {1} themes',
            language_switcher_label: 'Change language',
            community_allmods_subtitle: 'Browse and manage installed packages.',
            community_installmanager_subtitle: 'Manage isolated game profiles. Community never writes to the official Deltamod profile.',
            deleteall_description: 'This deletes only Deltamod Community data. Official Deltamod data is not touched.',
            locate_desc: 'To use Deltamod Community, locate a supported game installation.',
            main_empty_title: 'Your patch list is ready',
            main_empty_desc: 'Import a compatible mod package or browse the Mod Shop. Installed mods will appear here before anything touches the game files.',
            allmods_empty_title: 'No installed mods yet',
            allmods_empty_desc: 'Packages you download or import will stay visible here, even when they are not enabled in the current patch list.',
            browse_mod_shop: 'Browse Mod Shop',
            import_mod_package: 'Import mod package',
            community_delete_data_title: 'Delete all Community data',
            community_delete_data_desc: 'Deletes Community installations, mods, and options. Official Deltamod data is not changed.',
            community_hash_title: 'Enable hash checks',
            community_hash_desc: 'Checks mod hashes for compatibility. This may make scans slower.',
            community_dynamic_music_title: 'Enable dynamic music',
            community_dynamic_music_desc: 'Changes the background music based on the current page.',
            community_music_volume_title: 'Music volume',
            community_music_volume_desc: 'Adjusts the volume of menu and theme music.',
            community_alert_alignment: 'Alert alignment',
            community_seasonal_title: 'Seasonal details',
            community_seasonal_desc: 'Adds calendar-based pixel details without replacing the active theme. Choose an event to preview it.',
            seasonal_auto: 'Automatic',
            seasonal_off: 'Off',
            seasonal_womens_health: "Women's Health",
            seasonal_mens_health: "Men's Health",
            seasonal_easter: 'Easter',
            seasonal_halloween: 'Halloween',
            seasonal_christmas: 'Christmas',
            seasonal_new_year: 'New Year',
            theme_intro: 'Change the background, accent color, and menu music together.',
            theme_create: 'Create theme',
            theme_search: 'Search themes',
            theme_workshop: 'Theme workshop',
            theme_custom_title: 'Create a custom theme',
            theme_custom_description: 'Set its identity here, then choose the background and optional soundtrack.',
            theme_creation_steps: 'Creation steps',
            theme_step_identity: 'Identity',
            theme_step_background: 'Background',
            theme_step_music: 'Music',
            theme_name_placeholder: 'My theme',
            theme_description_label: 'Description',
            theme_optional: 'Optional',
            theme_description_placeholder: 'What is this theme based on?',
            theme_main_color: 'Main color',
            theme_color_aria: 'Theme main color',
            theme_color_help: 'Used for controls, accents, and the taskbar gear.',
            theme_soundtrack_title: 'Add a custom soundtrack',
            theme_soundtrack_help: 'MP3 or OGG. You will choose it after the background.',
            theme_icon_preview: 'Application icon preview',
            theme_taskbar_preview: 'Taskbar preview',
            theme_soul_color: 'SOUL color',
            theme_cancel: 'Cancel',
            theme_continue_background: 'Continue to background',
            theme_available: 'Available themes',
            theme_no_matches: 'No matching themes',
            theme_no_matches_hint: 'Try a different name, description, or music track.',
            theme_filter_placeholder: 'Name, description, or music',
            theme_background_preview: '{0} background preview',
            theme_accent_colors: 'UI accent: {0}; SOUL color: {1}',
            theme_accent_only: 'Theme accent: {0}',
            theme_built_in: 'Built-in',
            theme_custom: 'Custom',
            theme_credits: 'Credits',
            theme_edit_name: 'Click to edit the theme name',
            theme_edit_description: 'Click to edit the description',
            theme_in_use: 'In use',
            theme_use: 'Use theme',
            theme_delete: 'Delete',
            theme_delete_confirm: 'Delete "{0}"? This cannot be undone.',
            theme_name_required: 'Enter a name for the theme.',
            theme_choose_background: 'Choose a background image.',
            theme_choose_background_music: 'Choose a background image, then choose the music file.',
            theme_import_canceled: 'Import canceled. No theme files were copied.',
            theme_import_failed: 'Theme import failed: {0}',
            theme_unknown_error: 'Unknown error',
            allmods_unnamed: 'Unnamed mod',
            allmods_size: '{0} MB',
            allmods_no_id: 'No ID was specified.',
            allmods_variants: '{0} variants',
            allmods_compatible: 'Compatible with current version',
            allmods_incompatible: 'Incompatible: {0}',
            allmods_gamebanana: 'Installed through GameBanana',
            allmods_cant_like: "Can't like mod",
            allmods_already_liked: "You've already liked this mod. Can't get any more likes than that!",
            allmods_ok: 'OK',
            allmods_like_tooltip: 'Like this mod on GameBanana',
            allmods_comment_leave: 'Leave a comment on GameBanana',
            allmods_comment_view: 'View the GameBanana comments for this mod',
            allmods_mod_id: "Mod ID '{0}'",
            allmods_no_installation_match: 'No mods match this installation',
            allmods_choose_installation: 'Choose another installation above or return to All installations.'
        },
        it: {
            refine_load_failed: "Impossibile caricare le mod.",
            refine_retry: "Riprova",
            refine_search_mods: "Cerca mod",
            refine_search_hint: "Nome, autore, versione o ID del pacchetto",
            refine_clear: "Cancella",
            refine_mod_count: "{0} di {1} mod",
            refine_no_matches: "Nessuna mod corrispondente. Cancella la ricerca o cambia il filtro.",
            refine_saving: "Salvataggio…",
            refine_saved: "Salvato",
            refine_save_failed: "Non salvato. Riprova.",
            refine_progress: "Avanzamento",
            refine_patch_complete: "Patch completata!",
            refine_patch_log: "Registro della patch",

            community_options_subtitle: 'Configura Community senza modificare il profilo ufficiale di Deltamod.',
            optcat_data: 'Dati',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Scegli la lingua usata da Deltamod Community. Le nuove funzioni Community possono restare in inglese finché le traduzioni non vengono aggiornate.',
            language_current: 'Lingua attuale',
            language_select_hint: 'Seleziona lingua',
            language_restart_note: 'La pagina corrente si aggiorna subito dopo il cambio di lingua.',
            theme_count: '{0} di {1} temi',
            language_switcher_label: 'Cambia lingua',
            community_allmods_subtitle: 'Sfoglia e gestisci i pacchetti installati.',
            community_installmanager_subtitle: 'Gestisci profili di gioco isolati. Community non modifica mai il profilo ufficiale di Deltamod.',
            deleteall_description: 'Elimina solo i dati di Deltamod Community. I dati del Deltamod ufficiale non vengono modificati.',
            locate_desc: 'Per usare Deltamod Community, individua l’installazione di un gioco supportato.',
            main_empty_title: 'La lista delle patch è pronta',
            main_empty_desc: 'Importa un pacchetto compatibile o sfoglia il Mod Shop. Le mod installate appariranno qui prima che venga modificato qualsiasi file di gioco.',
            allmods_empty_title: 'Nessuna mod installata',
            allmods_empty_desc: 'I pacchetti scaricati o importati resteranno visibili qui anche quando non sono attivi nella lista delle patch.',
            browse_mod_shop: 'Sfoglia il Mod Shop',
            import_mod_package: 'Importa pacchetto mod',
            community_delete_data_title: 'Elimina tutti i dati Community',
            community_delete_data_desc: 'Elimina installazioni, mod e opzioni di Community. I dati del Deltamod ufficiale non vengono modificati.',
            community_hash_title: 'Abilita controllo degli hash',
            community_hash_desc: 'Controlla gli hash delle mod per verificarne la compatibilità. Le scansioni potrebbero essere più lente.',
            community_dynamic_music_title: 'Abilita musica dinamica',
            community_dynamic_music_desc: 'Cambia la musica di sottofondo in base alla pagina corrente.',
            community_music_volume_title: 'Volume musica',
            community_music_volume_desc: 'Regola il volume della musica dei menu e dei temi.',
            community_alert_alignment: 'Posizione degli avvisi',
            community_seasonal_title: 'Dettagli stagionali',
            community_seasonal_desc: 'Aggiunge dettagli pixel legati al calendario senza sostituire il tema attivo. Scegli un evento per visualizzarlo in anteprima.',
            seasonal_auto: 'Automatico',
            seasonal_off: 'Disattivato',
            seasonal_womens_health: 'Salute femminile',
            seasonal_mens_health: 'Salute maschile',
            seasonal_easter: 'Pasqua',
            seasonal_halloween: 'Halloween',
            seasonal_christmas: 'Natale',
            seasonal_new_year: 'Capodanno',
            theme_intro: 'Cambia insieme lo sfondo, il colore principale e la musica del menu.',
            theme_create: 'Crea tema',
            theme_search: 'Cerca temi',
            theme_workshop: 'Laboratorio temi',
            theme_custom_title: 'Crea un tema personalizzato',
            theme_custom_description: 'Definisci qui la sua identità, poi scegli lo sfondo e una colonna sonora facoltativa.',
            theme_creation_steps: 'Passaggi di creazione',
            theme_step_identity: 'Identità',
            theme_step_background: 'Sfondo',
            theme_step_music: 'Musica',
            theme_name_placeholder: 'Il mio tema',
            theme_description_label: 'Descrizione',
            theme_optional: 'Facoltativa',
            theme_description_placeholder: 'A cosa si ispira questo tema?',
            theme_main_color: 'Colore principale',
            theme_color_aria: 'Colore principale del tema',
            theme_color_help: 'Usato per i controlli, gli elementi in evidenza e l’ingranaggio della barra delle applicazioni.',
            theme_soundtrack_title: 'Aggiungi una colonna sonora personalizzata',
            theme_soundtrack_help: 'MP3 o OGG. Potrai sceglierla dopo lo sfondo.',
            theme_icon_preview: 'Anteprima dell’icona dell’applicazione',
            theme_taskbar_preview: 'Anteprima nella barra delle applicazioni',
            theme_soul_color: 'Colore dell’ANIMA',
            theme_cancel: 'Annulla',
            theme_continue_background: 'Continua allo sfondo',
            theme_available: 'Temi disponibili',
            theme_no_matches: 'Nessun tema corrispondente',
            theme_no_matches_hint: 'Prova un nome, una descrizione o una traccia musicale diversi.',
            theme_filter_placeholder: 'Nome, descrizione o musica',
            theme_background_preview: 'Anteprima dello sfondo di {0}',
            theme_accent_colors: 'Colore interfaccia: {0}; colore ANIMA: {1}',
            theme_accent_only: 'Colore del tema: {0}',
            theme_built_in: 'Integrato',
            theme_custom: 'Personalizzato',
            theme_credits: 'Riconoscimenti',
            theme_edit_name: 'Fai clic per modificare il nome del tema',
            theme_edit_description: 'Fai clic per modificare la descrizione',
            theme_in_use: 'In uso',
            theme_use: 'Usa tema',
            theme_delete: 'Elimina',
            theme_delete_confirm: 'Eliminare "{0}"? Questa azione non può essere annullata.',
            theme_name_required: 'Inserisci un nome per il tema.',
            theme_choose_background: 'Scegli un’immagine di sfondo.',
            theme_choose_background_music: 'Scegli un’immagine di sfondo, poi il file musicale.',
            theme_import_canceled: 'Importazione annullata. Nessun file del tema è stato copiato.',
            theme_import_failed: 'Importazione del tema non riuscita: {0}',
            theme_unknown_error: 'Errore sconosciuto',
            allmods_unnamed: 'Mod senza nome',
            allmods_size: '{0} MB',
            allmods_no_id: 'Non è stato specificato alcun ID.',
            allmods_variants: '{0} varianti',
            allmods_compatible: 'Compatibile con la versione attuale',
            allmods_incompatible: 'Non compatibile: {0}',
            allmods_gamebanana: 'Installata tramite GameBanana',
            allmods_cant_like: 'Impossibile mettere Mi piace alla mod',
            allmods_already_liked: 'Hai già messo Mi piace a questa mod. Non puoi farlo di nuovo!',
            allmods_ok: 'OK',
            allmods_like_tooltip: 'Metti Mi piace alla mod su GameBanana',
            allmods_comment_leave: 'Lascia un commento su GameBanana',
            allmods_comment_view: 'Visualizza i commenti della mod su GameBanana',
            allmods_mod_id: "ID mod '{0}'",
            allmods_no_installation_match: 'Nessuna mod corrisponde a questa installazione',
            allmods_choose_installation: 'Scegli un’altra installazione qui sopra o torna a Tutte le installazioni.'
        },
        pl: {
            refine_load_failed: "Nie udało się wczytać listy modów.",
            refine_retry: "Spróbuj ponownie",
            refine_search_mods: "Szukaj modów",
            refine_search_hint: "Nazwa, autor, wersja lub ID pakietu",
            refine_clear: "Wyczyść",
            refine_mod_count: "{0} z {1} modów",
            refine_no_matches: "Brak pasujących modów. Wyczyść wyszukiwanie lub zmień filtr.",
            refine_saving: "Zapisywanie…",
            refine_saved: "Zapisano",
            refine_save_failed: "Nie zapisano. Spróbuj ponownie.",
            refine_progress: "Postęp",
            refine_patch_complete: "Nakładanie modów zakończone!",
            refine_patch_log: "Dziennik zmian",

            community_options_subtitle: 'Skonfiguruj Community bez zmieniania oficjalnego profilu Deltamod.',
            optcat_data: 'Dane',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Wybierz język Deltamod Community. Nowe funkcje Community mogą pozostać po angielsku do czasu aktualizacji tłumaczeń.',
            language_current: 'Bieżący język',
            language_select_hint: 'Wybierz język',
            language_restart_note: 'Bieżąca strona odświeży się natychmiast po zmianie języka.',
            theme_count: '{0} z {1} motywów',
            language_switcher_label: 'Zmień język',
            community_allmods_subtitle: 'Przeglądaj zainstalowane pakiety i zarządzaj nimi.',
            community_installmanager_subtitle: 'Zarządzaj oddzielnymi profilami gier. Community nigdy nie modyfikuje oficjalnego profilu Deltamod.',
            deleteall_description: 'Usuwa tylko dane Deltamod Community. Dane oficjalnego Deltamod pozostają bez zmian.',
            locate_desc: 'Aby korzystać z Deltamod Community, wskaż instalację obsługiwanej gry.',
            main_empty_title: 'Lista patchy jest gotowa',
            main_empty_desc: 'Zaimportuj kompatybilny pakiet moda lub otwórz Sklep Modów. Zainstalowane mody pojawią się tutaj przed zmianą plików gry.',
            allmods_empty_title: 'Brak zainstalowanych modów',
            allmods_empty_desc: 'Pobrane lub zaimportowane pakiety pozostaną tutaj widoczne, nawet gdy nie są włączone na bieżącej liście patchy.',
            browse_mod_shop: 'Otwórz Sklep Modów',
            import_mod_package: 'Importuj pakiet moda',
            community_delete_data_title: 'Usuń wszystkie dane Community',
            community_delete_data_desc: 'Usuwa instalacje, mody i opcje Community. Dane oficjalnego Deltamod pozostają bez zmian.',
            community_hash_title: 'Włącz sprawdzanie hashy',
            community_hash_desc: 'Sprawdza hashe modów pod kątem kompatybilności. Skanowanie może potrwać dłużej.',
            community_dynamic_music_title: 'Włącz dynamiczną muzykę',
            community_dynamic_music_desc: 'Zmienia muzykę w tle zależnie od bieżącej strony.',
            community_music_volume_title: 'Głośność muzyki',
            community_music_volume_desc: 'Dostosowuje głośność muzyki menu i motywów.',
            community_alert_alignment: 'Położenie powiadomień',
            community_seasonal_title: 'Sezonowe detale',
            community_seasonal_desc: 'Dodaje pikselowe elementy zależne od kalendarza bez zastępowania aktywnego motywu. Wybierz wydarzenie, aby zobaczyć podgląd.',
            seasonal_auto: 'Automatycznie',
            seasonal_off: 'Wyłączone',
            seasonal_womens_health: 'Zdrowie kobiet',
            seasonal_mens_health: 'Zdrowie mężczyzn',
            seasonal_easter: 'Wielkanoc',
            seasonal_halloween: 'Halloween',
            seasonal_christmas: 'Boże Narodzenie',
            seasonal_new_year: 'Nowy Rok',
            theme_intro: 'Zmień jednocześnie tło, kolor akcentu i muzykę menu.',
            theme_create: 'Utwórz motyw',
            theme_search: 'Szukaj motywów',
            theme_workshop: 'Kreator motywów',
            theme_custom_title: 'Utwórz własny motyw',
            theme_custom_description: 'Określ tutaj jego tożsamość, a następnie wybierz tło i opcjonalną ścieżkę dźwiękową.',
            theme_creation_steps: 'Etapy tworzenia',
            theme_step_identity: 'Tożsamość',
            theme_step_background: 'Tło',
            theme_step_music: 'Muzyka',
            theme_name_placeholder: 'Mój motyw',
            theme_description_label: 'Opis',
            theme_optional: 'Opcjonalnie',
            theme_description_placeholder: 'Na czym opiera się ten motyw?',
            theme_main_color: 'Główny kolor',
            theme_color_aria: 'Główny kolor motywu',
            theme_color_help: 'Używany w elementach sterujących, akcentach i ikonie koła zębatego na pasku zadań.',
            theme_soundtrack_title: 'Dodaj własną ścieżkę dźwiękową',
            theme_soundtrack_help: 'MP3 lub OGG. Wybierzesz ją po wybraniu tła.',
            theme_icon_preview: 'Podgląd ikony aplikacji',
            theme_taskbar_preview: 'Podgląd na pasku zadań',
            theme_soul_color: 'Kolor DUSZY',
            theme_cancel: 'Anuluj',
            theme_continue_background: 'Przejdź do tła',
            theme_available: 'Dostępne motywy',
            theme_no_matches: 'Brak pasujących motywów',
            theme_no_matches_hint: 'Spróbuj innej nazwy, opisu lub utworu muzycznego.',
            theme_filter_placeholder: 'Nazwa, opis lub muzyka',
            theme_background_preview: 'Podgląd tła motywu {0}',
            theme_accent_colors: 'Akcent interfejsu: {0}; kolor DUSZY: {1}',
            theme_accent_only: 'Akcent motywu: {0}',
            theme_built_in: 'Wbudowany',
            theme_custom: 'Własny',
            theme_credits: 'Autorzy',
            theme_edit_name: 'Kliknij, aby edytować nazwę motywu',
            theme_edit_description: 'Kliknij, aby edytować opis',
            theme_in_use: 'Używany',
            theme_use: 'Użyj motywu',
            theme_delete: 'Usuń',
            theme_delete_confirm: 'Usunąć „{0}”? Tej operacji nie można cofnąć.',
            theme_name_required: 'Wpisz nazwę motywu.',
            theme_choose_background: 'Wybierz obraz tła.',
            theme_choose_background_music: 'Wybierz obraz tła, a następnie plik muzyczny.',
            theme_import_canceled: 'Importowanie anulowano. Nie skopiowano plików motywu.',
            theme_import_failed: 'Nie udało się zaimportować motywu: {0}',
            theme_unknown_error: 'Nieznany błąd',
            allmods_unnamed: 'Mod bez nazwy',
            allmods_size: '{0} MB',
            allmods_no_id: 'Nie podano identyfikatora.',
            allmods_variants: '{0} wariantów',
            allmods_compatible: 'Zgodny z bieżącą wersją',
            allmods_incompatible: 'Niezgodny: {0}',
            allmods_gamebanana: 'Zainstalowano przez GameBanana',
            allmods_cant_like: 'Nie można polubić moda',
            allmods_already_liked: 'Ten mod jest już przez Ciebie polubiony. Nie możesz polubić go ponownie!',
            allmods_ok: 'OK',
            allmods_like_tooltip: 'Polub ten mod na GameBanana',
            allmods_comment_leave: 'Dodaj komentarz na GameBanana',
            allmods_comment_view: 'Wyświetl komentarze tego moda na GameBanana',
            allmods_mod_id: "Identyfikator moda: '{0}'",
            allmods_no_installation_match: 'Brak modów dla tej instalacji',
            allmods_choose_installation: 'Wybierz powyżej inną instalację lub wróć do opcji Wszystkie instalacje.'
        },
        es: {
            refine_load_failed: "No se pudo cargar la lista de mods.",
            refine_retry: "Reintentar",
            refine_search_mods: "Buscar mods",
            refine_search_hint: "Nombre, autor, versión o ID del paquete",
            refine_clear: "Borrar",
            refine_mod_count: "{0} de {1} mods",
            refine_no_matches: "No hay mods coincidentes. Borra la búsqueda o cambia el filtro.",
            refine_saving: "Guardando…",
            refine_saved: "Guardado",
            refine_save_failed: "No se ha guardado. Inténtalo de nuevo.",
            refine_progress: "Progreso",
            refine_patch_complete: "¡Parche completado!",
            refine_patch_log: "Registro del parche",

            community_options_subtitle: 'Configura Community sin modificar el perfil oficial de Deltamod.',
            optcat_data: 'Datos',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Elige el idioma de Deltamod Community. Las funciones nuevas pueden mostrarse en inglés hasta que se actualice su traducción.',
            language_current: 'Idioma actual',
            language_select_hint: 'Seleccionar idioma',
            language_restart_note: 'La página actual se actualiza inmediatamente al cambiar de idioma.',
            theme_count: '{0} de {1} temas',
            language_switcher_label: 'Cambiar idioma',
            community_allmods_subtitle: 'Explora y administra los paquetes instalados.',
            community_installmanager_subtitle: 'Administra perfiles de juego aislados. Community nunca modifica el perfil oficial de Deltamod.',
            deleteall_description: 'Solo elimina los datos de Deltamod Community. Los datos del Deltamod oficial no se modifican.',
            locate_desc: 'Para usar Deltamod Community, localiza la instalación de un juego compatible.',
            main_empty_title: 'La lista de parches está lista',
            main_empty_desc: 'Importa un paquete compatible o explora el Mod Shop. Los mods instalados aparecerán aquí antes de modificar los archivos del juego.',
            allmods_empty_title: 'Todavía no hay mods instalados',
            allmods_empty_desc: 'Los paquetes descargados o importados permanecerán visibles aunque no estén activados en la lista de parches.',
            browse_mod_shop: 'Explorar Mod Shop',
            import_mod_package: 'Importar paquete de mod',
            community_delete_data_title: 'Eliminar todos los datos de Community',
            community_delete_data_desc: 'Elimina las instalaciones, mods y opciones de Community sin modificar los datos oficiales.',
            community_hash_title: 'Activar comprobación de hashes',
            community_hash_desc: 'Comprueba los hashes de los mods. Los análisis pueden tardar más.',
            community_dynamic_music_title: 'Activar música dinámica',
            community_dynamic_music_desc: 'Cambia la música de fondo según la página actual.',
            community_music_volume_title: 'Volumen de la música',
            community_music_volume_desc: 'Ajusta el volumen de la música de los menús y los temas.',
            community_alert_alignment: 'Posición de los avisos',
            community_seasonal_title: 'Detalles estacionales',
            community_seasonal_desc: 'Añade detalles de píxeles según el calendario sin reemplazar el tema activo. Elige un evento para previsualizarlo.',
            seasonal_auto: 'Automático',
            seasonal_off: 'Desactivado',
            seasonal_womens_health: 'Salud de la mujer',
            seasonal_mens_health: 'Salud del hombre',
            seasonal_easter: 'Pascua',
            seasonal_halloween: 'Halloween',
            seasonal_christmas: 'Navidad',
            seasonal_new_year: 'Año Nuevo',
            theme_intro: 'Cambia a la vez el fondo, el color de énfasis y la música del menú.',
            theme_create: 'Crear tema',
            theme_search: 'Buscar temas',
            theme_workshop: 'Taller de temas',
            theme_custom_title: 'Crear un tema personalizado',
            theme_custom_description: 'Define aquí su identidad y luego elige el fondo y una banda sonora opcional.',
            theme_creation_steps: 'Pasos de creación',
            theme_step_identity: 'Identidad',
            theme_step_background: 'Fondo',
            theme_step_music: 'Música',
            theme_name_placeholder: 'Mi tema',
            theme_description_label: 'Descripción',
            theme_optional: 'Opcional',
            theme_description_placeholder: '¿En qué se basa este tema?',
            theme_main_color: 'Color principal',
            theme_color_aria: 'Color principal del tema',
            theme_color_help: 'Se usa en controles, elementos destacados y el engranaje de la barra de tareas.',
            theme_soundtrack_title: 'Añadir una banda sonora personalizada',
            theme_soundtrack_help: 'MP3 u OGG. La elegirás después del fondo.',
            theme_icon_preview: 'Vista previa del icono de la aplicación',
            theme_taskbar_preview: 'Vista previa en la barra de tareas',
            theme_soul_color: 'Color del ALMA',
            theme_cancel: 'Cancelar',
            theme_continue_background: 'Continuar al fondo',
            theme_available: 'Temas disponibles',
            theme_no_matches: 'No hay temas coincidentes',
            theme_no_matches_hint: 'Prueba con otro nombre, descripción o pista musical.',
            theme_filter_placeholder: 'Nombre, descripción o música',
            theme_background_preview: 'Vista previa del fondo de {0}',
            theme_accent_colors: 'Énfasis de la interfaz: {0}; color del ALMA: {1}',
            theme_accent_only: 'Énfasis del tema: {0}',
            theme_built_in: 'Integrado',
            theme_custom: 'Personalizado',
            theme_credits: 'Créditos',
            theme_edit_name: 'Haz clic para editar el nombre del tema',
            theme_edit_description: 'Haz clic para editar la descripción',
            theme_in_use: 'En uso',
            theme_use: 'Usar tema',
            theme_delete: 'Eliminar',
            theme_delete_confirm: '¿Eliminar «{0}»? Esta acción no se puede deshacer.',
            theme_name_required: 'Introduce un nombre para el tema.',
            theme_choose_background: 'Elige una imagen de fondo.',
            theme_choose_background_music: 'Elige una imagen de fondo y luego el archivo de música.',
            theme_import_canceled: 'Importación cancelada. No se copió ningún archivo del tema.',
            theme_import_failed: 'Error al importar el tema: {0}',
            theme_unknown_error: 'Error desconocido',
            allmods_unnamed: 'Mod sin nombre',
            allmods_size: '{0} MB',
            allmods_no_id: 'No se especificó ningún ID.',
            allmods_variants: '{0} variantes',
            allmods_compatible: 'Compatible con la versión actual',
            allmods_incompatible: 'Incompatible: {0}',
            allmods_gamebanana: 'Instalado mediante GameBanana',
            allmods_cant_like: 'No se puede indicar que te gusta el mod',
            allmods_already_liked: 'Ya has indicado que te gusta este mod. ¡No puedes volver a hacerlo!',
            allmods_ok: 'Aceptar',
            allmods_like_tooltip: 'Indicar que te gusta este mod en GameBanana',
            allmods_comment_leave: 'Dejar un comentario en GameBanana',
            allmods_comment_view: 'Ver los comentarios de este mod en GameBanana',
            allmods_mod_id: "ID del mod: '{0}'",
            allmods_no_installation_match: 'No hay mods para esta instalación',
            allmods_choose_installation: 'Elige otra instalación arriba o vuelve a Todas las instalaciones.'
        },
        fr: {
            refine_load_failed: "Impossible de charger la liste des mods.",
            refine_retry: "Réessayer",
            refine_search_mods: "Rechercher des mods",
            refine_search_hint: "Nom, auteur, version ou ID du paquet",
            refine_clear: "Effacer",
            refine_mod_count: "{0} sur {1} mods",
            refine_no_matches: "Aucun mod correspondant. Effacez la recherche ou changez le filtre.",
            refine_saving: "Enregistrement…",
            refine_saved: "Enregistré",
            refine_save_failed: "Non enregistré. Réessayez.",
            refine_progress: "Progression",
            refine_patch_complete: "Application terminée !",
            refine_patch_log: "Journal des modifications",

            community_options_subtitle: 'Configurez Community sans modifier le profil officiel de Deltamod.',
            optcat_data: 'Données',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Choisissez la langue de Deltamod Community. Les nouvelles fonctions peuvent rester en anglais jusqu’à leur traduction.',
            language_current: 'Langue actuelle',
            language_select_hint: 'Choisir la langue',
            language_restart_note: 'La page actuelle s’actualise immédiatement après le changement de langue.',
            theme_count: '{0} sur {1} thèmes',
            language_switcher_label: 'Changer de langue',
            community_allmods_subtitle: 'Parcourez et gérez les paquets installés.',
            community_installmanager_subtitle: 'Gérez des profils de jeu isolés. Community ne modifie jamais le profil officiel de Deltamod.',
            deleteall_description: 'Supprime uniquement les données de Deltamod Community. Les données officielles restent intactes.',
            locate_desc: 'Pour utiliser Deltamod Community, localisez l’installation d’un jeu pris en charge.',
            main_empty_title: 'Votre liste de patchs est prête',
            main_empty_desc: 'Importez un paquet compatible ou parcourez le Mod Shop. Les mods installés apparaîtront ici avant toute modification des fichiers du jeu.',
            allmods_empty_title: 'Aucun mod installé',
            allmods_empty_desc: 'Les paquets téléchargés ou importés resteront visibles même s’ils ne sont pas activés dans la liste de patchs.',
            browse_mod_shop: 'Parcourir le Mod Shop',
            import_mod_package: 'Importer un paquet de mod',
            community_delete_data_title: 'Supprimer toutes les données Community',
            community_delete_data_desc: 'Supprime les installations, mods et options de Community sans modifier les données officielles.',
            community_hash_title: 'Activer la vérification des empreintes',
            community_hash_desc: 'Vérifie les empreintes des mods. Les analyses peuvent être plus lentes.',
            community_dynamic_music_title: 'Activer la musique dynamique',
            community_dynamic_music_desc: 'Adapte la musique de fond à la page actuelle.',
            community_music_volume_title: 'Volume de la musique',
            community_music_volume_desc: 'Règle le volume de la musique des menus et des thèmes.',
            community_alert_alignment: 'Position des alertes',
            community_seasonal_title: 'Détails saisonniers',
            community_seasonal_desc: 'Ajoute des détails en pixel art selon le calendrier sans remplacer le thème actif. Choisissez un événement pour l’aperçu.',
            seasonal_auto: 'Automatique',
            seasonal_off: 'Désactivé',
            seasonal_womens_health: 'Santé des femmes',
            seasonal_mens_health: 'Santé des hommes',
            seasonal_easter: 'Pâques',
            seasonal_halloween: 'Halloween',
            seasonal_christmas: 'Noël',
            seasonal_new_year: 'Nouvel An',
            theme_intro: 'Changez simultanément l’arrière-plan, la couleur d’accentuation et la musique du menu.',
            theme_create: 'Créer un thème',
            theme_search: 'Rechercher des thèmes',
            theme_workshop: 'Atelier de thèmes',
            theme_custom_title: 'Créer un thème personnalisé',
            theme_custom_description: 'Définissez ici son identité, puis choisissez l’arrière-plan et une bande-son facultative.',
            theme_creation_steps: 'Étapes de création',
            theme_step_identity: 'Identité',
            theme_step_background: 'Arrière-plan',
            theme_step_music: 'Musique',
            theme_name_placeholder: 'Mon thème',
            theme_description_label: 'Description',
            theme_optional: 'Facultatif',
            theme_description_placeholder: 'De quoi ce thème s’inspire-t-il ?',
            theme_main_color: 'Couleur principale',
            theme_color_aria: 'Couleur principale du thème',
            theme_color_help: 'Utilisée pour les commandes, les accents et l’engrenage de la barre des tâches.',
            theme_soundtrack_title: 'Ajouter une bande-son personnalisée',
            theme_soundtrack_help: 'MP3 ou OGG. Vous la choisirez après l’arrière-plan.',
            theme_icon_preview: 'Aperçu de l’icône de l’application',
            theme_taskbar_preview: 'Aperçu dans la barre des tâches',
            theme_soul_color: 'Couleur de l’ÂME',
            theme_cancel: 'Annuler',
            theme_continue_background: 'Continuer vers l’arrière-plan',
            theme_available: 'Thèmes disponibles',
            theme_no_matches: 'Aucun thème correspondant',
            theme_no_matches_hint: 'Essayez un autre nom, une autre description ou une autre piste musicale.',
            theme_filter_placeholder: 'Nom, description ou musique',
            theme_background_preview: 'Aperçu de l’arrière-plan de {0}',
            theme_accent_colors: 'Accent de l’interface : {0} ; couleur de l’ÂME : {1}',
            theme_accent_only: 'Accent du thème : {0}',
            theme_built_in: 'Intégré',
            theme_custom: 'Personnalisé',
            theme_credits: 'Crédits',
            theme_edit_name: 'Cliquez pour modifier le nom du thème',
            theme_edit_description: 'Cliquez pour modifier la description',
            theme_in_use: 'Utilisé',
            theme_use: 'Utiliser ce thème',
            theme_delete: 'Supprimer',
            theme_delete_confirm: 'Supprimer « {0} » ? Cette action est irréversible.',
            theme_name_required: 'Saisissez un nom pour le thème.',
            theme_choose_background: 'Choisissez une image d’arrière-plan.',
            theme_choose_background_music: 'Choisissez une image d’arrière-plan, puis le fichier musical.',
            theme_import_canceled: 'Importation annulée. Aucun fichier du thème n’a été copié.',
            theme_import_failed: 'Échec de l’importation du thème : {0}',
            theme_unknown_error: 'Erreur inconnue',
            allmods_unnamed: 'Mod sans nom',
            allmods_size: '{0} Mo',
            allmods_no_id: 'Aucun identifiant n’a été indiqué.',
            allmods_variants: '{0} variantes',
            allmods_compatible: 'Compatible avec la version actuelle',
            allmods_incompatible: 'Incompatible : {0}',
            allmods_gamebanana: 'Installé via GameBanana',
            allmods_cant_like: 'Impossible d’aimer ce mod',
            allmods_already_liked: 'Vous aimez déjà ce mod. Impossible de l’aimer une seconde fois !',
            allmods_ok: 'OK',
            allmods_like_tooltip: 'Aimer ce mod sur GameBanana',
            allmods_comment_leave: 'Laisser un commentaire sur GameBanana',
            allmods_comment_view: 'Voir les commentaires de ce mod sur GameBanana',
            allmods_mod_id: "Identifiant du mod : '{0}'",
            allmods_no_installation_match: 'Aucun mod pour cette installation',
            allmods_choose_installation: 'Choisissez une autre installation ci-dessus ou revenez à Toutes les installations.'
        },
        de: {
            refine_load_failed: "Die Mod-Liste konnte nicht geladen werden.",
            refine_retry: "Erneut versuchen",
            refine_search_mods: "Mods suchen",
            refine_search_hint: "Name, Autor, Version oder Paket-ID",
            refine_clear: "Leeren",
            refine_mod_count: "{0} von {1} Mods",
            refine_no_matches: "Keine passenden Mods. Suche leeren oder Filter ändern.",
            refine_saving: "Speichern…",
            refine_saved: "Gespeichert",
            refine_save_failed: "Nicht gespeichert. Erneut versuchen.",
            refine_progress: "Fortschritt",
            refine_patch_complete: "Patchen abgeschlossen!",
            refine_patch_log: "Patch-Protokoll",

            community_options_subtitle: 'Community konfigurieren, ohne das offizielle Deltamod-Profil zu verändern.',
            optcat_data: 'Daten',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Wähle die Sprache von Deltamod Community. Neue Funktionen können bis zur Übersetzung auf Englisch angezeigt werden.',
            language_current: 'Aktuelle Sprache',
            language_select_hint: 'Sprache auswählen',
            language_restart_note: 'Die aktuelle Seite wird nach dem Sprachwechsel sofort neu geladen.',
            theme_count: '{0} von {1} Themes',
            language_switcher_label: 'Sprache ändern',
            community_allmods_subtitle: 'Installierte Pakete durchsuchen und verwalten.',
            community_installmanager_subtitle: 'Getrennte Spielprofile verwalten. Community verändert niemals das offizielle Deltamod-Profil.',
            deleteall_description: 'Löscht nur Daten von Deltamod Community. Die offiziellen Deltamod-Daten bleiben unverändert.',
            locate_desc: 'Wähle eine unterstützte Spielinstallation aus, um Deltamod Community zu verwenden.',
            main_empty_title: 'Deine Patch-Liste ist bereit',
            main_empty_desc: 'Importiere ein kompatibles Mod-Paket oder öffne den Mod Shop. Installierte Mods erscheinen hier, bevor Spieldateien verändert werden.',
            allmods_empty_title: 'Noch keine Mods installiert',
            allmods_empty_desc: 'Heruntergeladene oder importierte Pakete bleiben sichtbar, auch wenn sie nicht in der Patch-Liste aktiviert sind.',
            browse_mod_shop: 'Mod Shop öffnen',
            import_mod_package: 'Mod-Paket importieren',
            community_delete_data_title: 'Alle Community-Daten löschen',
            community_delete_data_desc: 'Löscht Community-Installationen, Mods und Optionen, ohne offizielle Daten zu verändern.',
            community_hash_title: 'Hash-Prüfungen aktivieren',
            community_hash_desc: 'Prüft Mod-Hashes auf Kompatibilität. Scans können dadurch länger dauern.',
            community_dynamic_music_title: 'Dynamische Musik aktivieren',
            community_dynamic_music_desc: 'Passt die Hintergrundmusik an die aktuelle Seite an.',
            community_music_volume_title: 'Musiklautstärke',
            community_music_volume_desc: 'Regelt die Lautstärke der Menü- und Theme-Musik.',
            community_alert_alignment: 'Position der Hinweise',
            community_seasonal_title: 'Saisonale Details',
            community_seasonal_desc: 'Ergänzt kalenderabhängige Pixel-Details, ohne das aktive Theme zu ersetzen. Wähle ein Ereignis für die Vorschau.',
            seasonal_auto: 'Automatisch',
            seasonal_off: 'Aus',
            seasonal_womens_health: 'Frauengesundheit',
            seasonal_mens_health: 'Männergesundheit',
            seasonal_easter: 'Ostern',
            seasonal_halloween: 'Halloween',
            seasonal_christmas: 'Weihnachten',
            seasonal_new_year: 'Neujahr',
            theme_intro: 'Ändere Hintergrund, Akzentfarbe und Menümusik gemeinsam.',
            theme_create: 'Theme erstellen',
            theme_search: 'Themes suchen',
            theme_workshop: 'Theme-Werkstatt',
            theme_custom_title: 'Eigenes Theme erstellen',
            theme_custom_description: 'Lege hier seine Identität fest und wähle danach den Hintergrund und optional einen Soundtrack aus.',
            theme_creation_steps: 'Erstellungsschritte',
            theme_step_identity: 'Identität',
            theme_step_background: 'Hintergrund',
            theme_step_music: 'Musik',
            theme_name_placeholder: 'Mein Theme',
            theme_description_label: 'Beschreibung',
            theme_optional: 'Optional',
            theme_description_placeholder: 'Worauf basiert dieses Theme?',
            theme_main_color: 'Hauptfarbe',
            theme_color_aria: 'Hauptfarbe des Themes',
            theme_color_help: 'Wird für Steuerelemente, Akzente und das Zahnrad in der Taskleiste verwendet.',
            theme_soundtrack_title: 'Eigenen Soundtrack hinzufügen',
            theme_soundtrack_help: 'MP3 oder OGG. Du wählst ihn nach dem Hintergrund aus.',
            theme_icon_preview: 'Vorschau des Anwendungssymbols',
            theme_taskbar_preview: 'Taskleistenvorschau',
            theme_soul_color: 'SEELEN-Farbe',
            theme_cancel: 'Abbrechen',
            theme_continue_background: 'Weiter zum Hintergrund',
            theme_available: 'Verfügbare Themes',
            theme_no_matches: 'Keine passenden Themes',
            theme_no_matches_hint: 'Versuche es mit einem anderen Namen, einer anderen Beschreibung oder einem anderen Musiktitel.',
            theme_filter_placeholder: 'Name, Beschreibung oder Musik',
            theme_background_preview: 'Hintergrundvorschau für {0}',
            theme_accent_colors: 'Oberflächenakzent: {0}; SEELEN-Farbe: {1}',
            theme_accent_only: 'Theme-Akzent: {0}',
            theme_built_in: 'Integriert',
            theme_custom: 'Benutzerdefiniert',
            theme_credits: 'Mitwirkende',
            theme_edit_name: 'Klicken, um den Namen des Themes zu bearbeiten',
            theme_edit_description: 'Klicken, um die Beschreibung zu bearbeiten',
            theme_in_use: 'In Verwendung',
            theme_use: 'Theme verwenden',
            theme_delete: 'Löschen',
            theme_delete_confirm: '„{0}“ löschen? Dies kann nicht rückgängig gemacht werden.',
            theme_name_required: 'Gib einen Namen für das Theme ein.',
            theme_choose_background: 'Wähle ein Hintergrundbild aus.',
            theme_choose_background_music: 'Wähle ein Hintergrundbild und danach die Musikdatei aus.',
            theme_import_canceled: 'Import abgebrochen. Es wurden keine Theme-Dateien kopiert.',
            theme_import_failed: 'Theme-Import fehlgeschlagen: {0}',
            theme_unknown_error: 'Unbekannter Fehler',
            allmods_unnamed: 'Unbenannter Mod',
            allmods_size: '{0} MB',
            allmods_no_id: 'Es wurde keine ID angegeben.',
            allmods_variants: '{0} Varianten',
            allmods_compatible: 'Mit der aktuellen Version kompatibel',
            allmods_incompatible: 'Nicht kompatibel: {0}',
            allmods_gamebanana: 'Über GameBanana installiert',
            allmods_cant_like: 'Mod kann nicht gelikt werden',
            allmods_already_liked: 'Du hast diesen Mod bereits gelikt. Mehr als einmal geht nicht!',
            allmods_ok: 'OK',
            allmods_like_tooltip: 'Diesen Mod auf GameBanana liken',
            allmods_comment_leave: 'Kommentar auf GameBanana hinterlassen',
            allmods_comment_view: 'GameBanana-Kommentare zu diesem Mod anzeigen',
            allmods_mod_id: "Mod-ID '{0}'",
            allmods_no_installation_match: 'Keine Mods für diese Installation',
            allmods_choose_installation: 'Wähle oben eine andere Installation aus oder kehre zu Alle Installationen zurück.'
        },
        'pt-br': {
            refine_load_failed: "Não foi possível carregar a lista de mods.",
            refine_retry: "Tentar novamente",
            refine_search_mods: "Buscar mods",
            refine_search_hint: "Nome, autor, versão ou ID do pacote",
            refine_clear: "Limpar",
            refine_mod_count: "{0} de {1} mods",
            refine_no_matches: "Nenhum mod encontrado. Limpe a busca ou altere o filtro.",
            refine_saving: "Salvando…",
            refine_saved: "Salvo",
            refine_save_failed: "Não foi salvo. Tente novamente.",
            refine_progress: "Progresso",
            refine_patch_complete: "Aplicação concluída!",
            refine_patch_log: "Registro de aplicação",

            community_options_subtitle: 'Configure o Community sem alterar o perfil oficial do Deltamod.',
            optcat_data: 'Dados',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Escolha o idioma do Deltamod Community. Recursos novos podem permanecer em inglês até que a tradução seja atualizada.',
            language_current: 'Idioma atual',
            language_select_hint: 'Selecionar idioma',
            language_restart_note: 'A página atual é atualizada imediatamente após a troca de idioma.',
            theme_count: '{0} de {1} temas',
            language_switcher_label: 'Alterar idioma',
            community_allmods_subtitle: 'Navegue e gerencie os pacotes instalados.',
            community_installmanager_subtitle: 'Gerencie perfis de jogo isolados. O Community nunca altera o perfil oficial do Deltamod.',
            deleteall_description: 'Exclui apenas os dados do Deltamod Community. Os dados oficiais permanecem intactos.',
            locate_desc: 'Para usar o Deltamod Community, localize a instalação de um jogo compatível.',
            main_empty_title: 'Sua lista de patches está pronta',
            main_empty_desc: 'Importe um pacote compatível ou navegue pelo Mod Shop. Os mods instalados aparecerão aqui antes de qualquer arquivo do jogo ser alterado.',
            allmods_empty_title: 'Nenhum mod instalado',
            allmods_empty_desc: 'Pacotes baixados ou importados continuarão visíveis mesmo quando não estiverem ativados na lista de patches.',
            browse_mod_shop: 'Abrir Mod Shop',
            import_mod_package: 'Importar pacote de mod',
            community_delete_data_title: 'Excluir todos os dados do Community',
            community_delete_data_desc: 'Exclui instalações, mods e opções do Community sem alterar os dados oficiais.',
            community_hash_title: 'Ativar verificação de hashes',
            community_hash_desc: 'Verifica os hashes dos mods. As análises podem demorar mais.',
            community_dynamic_music_title: 'Ativar música dinâmica',
            community_dynamic_music_desc: 'Altera a música de fundo conforme a página atual.',
            community_music_volume_title: 'Volume da música',
            community_music_volume_desc: 'Ajusta o volume da música dos menus e temas.',
            community_alert_alignment: 'Posição dos alertas',
            community_seasonal_title: 'Detalhes sazonais',
            community_seasonal_desc: 'Adiciona detalhes em pixel art conforme o calendário sem substituir o tema ativo. Escolha um evento para visualizar.',
            seasonal_auto: 'Automático',
            seasonal_off: 'Desativado',
            seasonal_womens_health: 'Saúde da mulher',
            seasonal_mens_health: 'Saúde do homem',
            seasonal_easter: 'Páscoa',
            seasonal_halloween: 'Halloween',
            seasonal_christmas: 'Natal',
            seasonal_new_year: 'Ano Novo',
            theme_intro: 'Altere o plano de fundo, a cor de destaque e a música do menu em conjunto.',
            theme_create: 'Criar tema',
            theme_search: 'Pesquisar temas',
            theme_workshop: 'Oficina de temas',
            theme_custom_title: 'Criar um tema personalizado',
            theme_custom_description: 'Defina a identidade aqui e depois escolha o plano de fundo e uma trilha sonora opcional.',
            theme_creation_steps: 'Etapas de criação',
            theme_step_identity: 'Identidade',
            theme_step_background: 'Plano de fundo',
            theme_step_music: 'Música',
            theme_name_placeholder: 'Meu tema',
            theme_description_label: 'Descrição',
            theme_optional: 'Opcional',
            theme_description_placeholder: 'Em que este tema se baseia?',
            theme_main_color: 'Cor principal',
            theme_color_aria: 'Cor principal do tema',
            theme_color_help: 'Usada nos controles, destaques e engrenagem da barra de tarefas.',
            theme_soundtrack_title: 'Adicionar uma trilha sonora personalizada',
            theme_soundtrack_help: 'MP3 ou OGG. Você poderá escolhê-la depois do plano de fundo.',
            theme_icon_preview: 'Prévia do ícone do aplicativo',
            theme_taskbar_preview: 'Prévia na barra de tarefas',
            theme_soul_color: 'Cor da ALMA',
            theme_cancel: 'Cancelar',
            theme_continue_background: 'Continuar para o plano de fundo',
            theme_available: 'Temas disponíveis',
            theme_no_matches: 'Nenhum tema correspondente',
            theme_no_matches_hint: 'Tente outro nome, descrição ou faixa de música.',
            theme_filter_placeholder: 'Nome, descrição ou música',
            theme_background_preview: 'Prévia do plano de fundo de {0}',
            theme_accent_colors: 'Destaque da interface: {0}; cor da ALMA: {1}',
            theme_accent_only: 'Destaque do tema: {0}',
            theme_built_in: 'Integrado',
            theme_custom: 'Personalizado',
            theme_credits: 'Créditos',
            theme_edit_name: 'Clique para editar o nome do tema',
            theme_edit_description: 'Clique para editar a descrição',
            theme_in_use: 'Em uso',
            theme_use: 'Usar tema',
            theme_delete: 'Excluir',
            theme_delete_confirm: 'Excluir “{0}”? Esta ação não pode ser desfeita.',
            theme_name_required: 'Digite um nome para o tema.',
            theme_choose_background: 'Escolha uma imagem de plano de fundo.',
            theme_choose_background_music: 'Escolha uma imagem de plano de fundo e depois o arquivo de música.',
            theme_import_canceled: 'Importação cancelada. Nenhum arquivo do tema foi copiado.',
            theme_import_failed: 'Falha ao importar o tema: {0}',
            theme_unknown_error: 'Erro desconhecido',
            allmods_unnamed: 'Mod sem nome',
            allmods_size: '{0} MB',
            allmods_no_id: 'Nenhum ID foi especificado.',
            allmods_variants: '{0} variantes',
            allmods_compatible: 'Compatível com a versão atual',
            allmods_incompatible: 'Incompatível: {0}',
            allmods_gamebanana: 'Instalado pelo GameBanana',
            allmods_cant_like: 'Não foi possível curtir o mod',
            allmods_already_liked: 'Você já curtiu este mod. Não é possível curtir novamente!',
            allmods_ok: 'OK',
            allmods_like_tooltip: 'Curtir este mod no GameBanana',
            allmods_comment_leave: 'Deixar um comentário no GameBanana',
            allmods_comment_view: 'Ver os comentários deste mod no GameBanana',
            allmods_mod_id: "ID do mod: '{0}'",
            allmods_no_installation_match: 'Nenhum mod para esta instalação',
            allmods_choose_installation: 'Escolha outra instalação acima ou volte para Todas as instalações.'
        },
        ja: {
            refine_load_failed: "MODリストを読み込めませんでした。",
            refine_retry: "再試行",
            refine_search_mods: "MODを検索",
            refine_search_hint: "名前、作者、バージョン、パッケージID",
            refine_clear: "クリア",
            refine_mod_count: "{1}件中{0}件のMOD",
            refine_no_matches: "一致するMODがありません。検索をクリアするか、フィルターを変更してください。",
            refine_saving: "保存中…",
            refine_saved: "保存しました",
            refine_save_failed: "保存できませんでした。もう一度お試しください。",
            refine_progress: "進行状況",
            refine_patch_complete: "パッチ完了！",
            refine_patch_log: "パッチログ",

            community_options_subtitle: '公式Deltamodのプロフィールを変更せずにCommunityを設定します。',
            optcat_data: 'データ',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Deltamod Communityで使用する言語を選択します。新機能は翻訳されるまで英語で表示される場合があります。',
            language_current: '現在の言語',
            language_select_hint: '言語を選択',
            language_restart_note: '言語を変更すると、現在のページがすぐに更新されます。',
            theme_count: '{1}件中{0}件のテーマ',
            language_switcher_label: '言語を変更',
            community_allmods_subtitle: 'インストール済みパッケージを確認・管理します。',
            community_installmanager_subtitle: '個別のゲームプロフィールを管理します。Communityは公式Deltamodのプロフィールを変更しません。',
            deleteall_description: 'Deltamod Communityのデータのみを削除します。公式Deltamodのデータは変更されません。',
            locate_desc: 'Deltamod Communityを使用するには、対応するゲームのインストール先を選択してください。',
            main_empty_title: 'パッチリストの準備ができました',
            main_empty_desc: '対応するMODパッケージをインポートするか、Mod Shopを開いてください。ゲームファイルを変更する前に、インストール済みMODがここに表示されます。',
            allmods_empty_title: 'MODはまだインストールされていません',
            allmods_empty_desc: 'ダウンロードまたはインポートしたパッケージは、パッチリストで無効にしてもここに表示されます。',
            browse_mod_shop: 'Mod Shopを開く',
            import_mod_package: 'MODパッケージをインポート',
            community_delete_data_title: 'Communityの全データを削除',
            community_delete_data_desc: '公式データを変更せず、Communityのインストール、MOD、オプションを削除します。',
            community_hash_title: 'ハッシュチェックを有効にする',
            community_hash_desc: 'MODのハッシュを確認します。スキャンに時間がかかる場合があります。',
            community_dynamic_music_title: 'ダイナミック音楽を有効にする',
            community_dynamic_music_desc: '現在のページに応じてBGMを変更します。',
            community_music_volume_title: '音楽の音量',
            community_music_volume_desc: 'メニューとテーマの音楽の音量を調整します。',
            community_alert_alignment: '通知の位置',
            community_seasonal_title: '季節の演出',
            community_seasonal_desc: '選択中のテーマを置き換えず、カレンダーに合わせたピクセル演出を追加します。イベントを選ぶとプレビューできます。',
            seasonal_auto: '自動',
            seasonal_off: 'オフ',
            seasonal_womens_health: '女性の健康',
            seasonal_mens_health: '男性の健康',
            seasonal_easter: 'イースター',
            seasonal_halloween: 'ハロウィン',
            seasonal_christmas: 'クリスマス',
            seasonal_new_year: '新年',
            theme_intro: '背景、アクセントカラー、メニュー音楽をまとめて変更します。',
            theme_create: 'テーマを作成',
            theme_search: 'テーマを検索',
            theme_workshop: 'テーマ作成',
            theme_custom_title: 'カスタムテーマを作成',
            theme_custom_description: 'ここでテーマの基本情報を設定し、背景と任意のサウンドトラックを選択します。',
            theme_creation_steps: '作成手順',
            theme_step_identity: '基本情報',
            theme_step_background: '背景',
            theme_step_music: '音楽',
            theme_name_placeholder: 'マイテーマ',
            theme_description_label: '説明',
            theme_optional: '任意',
            theme_description_placeholder: 'このテーマは何をモチーフにしていますか？',
            theme_main_color: 'メインカラー',
            theme_color_aria: 'テーマのメインカラー',
            theme_color_help: '操作部、アクセント、タスクバーの歯車に使用されます。',
            theme_soundtrack_title: 'カスタムサウンドトラックを追加',
            theme_soundtrack_help: 'MP3またはOGG。背景を選んだ後に選択します。',
            theme_icon_preview: 'アプリケーションアイコンのプレビュー',
            theme_taskbar_preview: 'タスクバーのプレビュー',
            theme_soul_color: 'ソウルカラー',
            theme_cancel: 'キャンセル',
            theme_continue_background: '背景の選択へ',
            theme_available: '利用可能なテーマ',
            theme_no_matches: '一致するテーマがありません',
            theme_no_matches_hint: '別の名前、説明、または曲名を試してください。',
            theme_filter_placeholder: '名前、説明、または音楽',
            theme_background_preview: '{0}の背景プレビュー',
            theme_accent_colors: 'UIアクセント：{0}、ソウルカラー：{1}',
            theme_accent_only: 'テーマアクセント：{0}',
            theme_built_in: '内蔵',
            theme_custom: 'カスタム',
            theme_credits: 'クレジット',
            theme_edit_name: 'クリックしてテーマ名を編集',
            theme_edit_description: 'クリックして説明を編集',
            theme_in_use: '使用中',
            theme_use: 'テーマを使用',
            theme_delete: '削除',
            theme_delete_confirm: '「{0}」を削除しますか？この操作は元に戻せません。',
            theme_name_required: 'テーマ名を入力してください。',
            theme_choose_background: '背景画像を選択してください。',
            theme_choose_background_music: '背景画像を選択してから、音楽ファイルを選択してください。',
            theme_import_canceled: 'インポートをキャンセルしました。テーマファイルはコピーされていません。',
            theme_import_failed: 'テーマのインポートに失敗しました：{0}',
            theme_unknown_error: '不明なエラー',
            allmods_unnamed: '名前のないMOD',
            allmods_size: '{0} MB',
            allmods_no_id: 'IDが指定されていません。',
            allmods_variants: '{0}個のバリエーション',
            allmods_compatible: '現在のバージョンに対応',
            allmods_incompatible: '非対応：{0}',
            allmods_gamebanana: 'GameBananaからインストール済み',
            allmods_cant_like: 'MODに「いいね」できません',
            allmods_already_liked: 'このMODにはすでに「いいね」しています。もう一度「いいね」することはできません！',
            allmods_ok: 'OK',
            allmods_like_tooltip: 'GameBananaでこのMODに「いいね」する',
            allmods_comment_leave: 'GameBananaにコメントを投稿',
            allmods_comment_view: 'GameBananaでこのMODのコメントを表示',
            allmods_mod_id: "MOD ID「{0}」",
            allmods_no_installation_match: 'このインストールに該当するMODはありません',
            allmods_choose_installation: '上で別のインストールを選ぶか、「すべてのインストール」に戻ってください。'
        }
    });

    const communityKeys = Object.keys(communityStrings.en).sort();
    const placeholders = value => [...String(value).matchAll(/{(\d+)}/g)]
        .map(match => match[0])
        .sort();
    for (const code of SUPPORTED_LANGUAGES) {
        const catalog = communityStrings[code];
        if (!catalog || JSON.stringify(Object.keys(catalog).sort()) !== JSON.stringify(communityKeys)) {
            throw new Error(`Community localization keys do not match English for ${code}`);
        }
        for (const key of communityKeys) {
            if (JSON.stringify(placeholders(catalog[key])) !== JSON.stringify(placeholders(communityStrings.en[key]))) {
                throw new Error(`Community localization placeholders do not match English for ${code}:${key}`);
            }
        }
    }

    let currentLanguage = normalizeLanguage(
        localStorage.getItem(STORAGE_KEY) || navigator.language
    );
    let reverseDictionary = new Map();
    const knownTextNodes = new Map();
    const knownAttributes = new Map();

    function normalizeLanguage(value) {
        const code = String(value || '').trim().toLowerCase();
        if (SUPPORTED_LANGUAGES.includes(code)) return code;
        if (code.startsWith('pt')) return 'pt-br';
        const base = code.split('-')[0];
        return SUPPORTED_LANGUAGES.includes(base) ? base : DEFAULT_LANGUAGE;
    }

    function stripJsonComments(value) {
        return value.replace(/\/\*[\s\S]*?\*\//g, '');
    }

    async function fetchLanguageFile(code, filename) {
        const response = await fetch(`./langs/${code}/${filename}`);
        if (!response.ok) {
            throw new Error(`Unable to load ${code}/${filename}: HTTP ${response.status}`);
        }
        return response.text();
    }

    async function loadDictionary(code) {
        if (dictionaries.has(code)) return dictionaries.get(code);
        const text = await fetchLanguageFile(code, 'language.json');
        const parsed = JSON.parse(stripJsonComments(text));
        const merged = Object.freeze({
            ...parsed,
            ...(communityStrings[code] || {})
        });
        dictionaries.set(code, merged);
        return merged;
    }

    async function loadMetadata(code) {
        if (metadata.has(code)) return metadata.get(code);
        const lines = (await fetchLanguageFile(code, 'metadata.txt'))
            .split(/\r?\n/)
            .map(line => line.trim());
        const entry = Object.freeze({
            code,
            name: lines[0] || code.toUpperCase(),
            author: lines[1] || 'Unknown',
            version: lines[2] || 'Unknown',
            flag: `./langs/${code}/${
                /^[A-Za-z0-9._-]+$/.test(lines[3] || '') ? lines[3] : 'flag.png'
            }`
        });
        metadata.set(code, entry);
        return entry;
    }

    function interpolate(value, args) {
        return String(value).replace(/{(\d+)}/g, (match, index) => (
            args[index] === undefined ? match : String(args[index])
        ));
    }

    function rebuildReverseDictionary() {
        reverseDictionary = new Map();
        const english = dictionaries.get(DEFAULT_LANGUAGE) || {};
        for (const [key, englishValue] of Object.entries(english)) {
            if (typeof englishValue !== 'string' || englishValue.includes('{')) continue;
            for (const dictionary of dictionaries.values()) {
                const knownValue = dictionary[key];
                if (typeof knownValue !== 'string') continue;
                if (!reverseDictionary.has(knownValue)) {
                    reverseDictionary.set(knownValue, key);
                } else if (reverseDictionary.get(knownValue) !== key) {
                    reverseDictionary.set(knownValue, null);
                }
            }
        }
    }

    async function initialize() {
        await Promise.all([
            loadDictionary(DEFAULT_LANGUAGE),
            loadDictionary(currentLanguage)
        ]);
        await Promise.all(SUPPORTED_LANGUAGES.map(async code => {
            try {
                await loadMetadata(code);
            } catch (error) {
                console.warn(`Unable to load metadata for ${code}:`, error);
                metadata.set(code, Object.freeze({
                    code,
                    name: code.toUpperCase(),
                    author: 'Unknown',
                    version: 'Unknown',
                    flag: `./langs/${code}/flag.png`
                }));
            }
        }));
        rebuildReverseDictionary();
        document.documentElement.lang = currentLanguage;
        applyKnownText(document);
    }

    function t(key, fallback = key, ...args) {
        const selected = dictionaries.get(currentLanguage) || {};
        const english = dictionaries.get(DEFAULT_LANGUAGE) || {};
        return interpolate(selected[key] ?? english[key] ?? fallback, args);
    }

    function translateKnownText(value) {
        if (typeof value !== 'string') return value;
        const key = reverseDictionary.get(value);
        return key ? t(key, value) : value;
    }

    function replaceTokens(html) {
        return String(html).replace(/\$\$(.*?)\$\$/g, (_match, key) => t(key, `$$${key}$$`));
    }

    function apply(root = document) {
        root.querySelectorAll('[data-i18n]').forEach(element => {
            element.textContent = t(element.dataset.i18n, element.textContent);
        });
        root.querySelectorAll('[data-i18n-title]').forEach(element => {
            element.title = t(element.dataset.i18nTitle, element.title);
        });
        root.querySelectorAll('[data-i18n-aria-label]').forEach(element => {
            element.setAttribute(
                'aria-label',
                t(element.dataset.i18nAriaLabel, element.getAttribute('aria-label') || '')
            );
        });
    }

    function applyKnownText(root = document) {
        const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
        const nodes = [];
        while (walker.nextNode()) nodes.push(walker.currentNode);
        for (const node of nodes) {
            if (knownTextNodes.has(node)
                || node.parentElement?.closest('script, style, textarea, [contenteditable="true"]')) continue;
            const original = node.nodeValue || '';
            const content = original.trim();
            if (!content) continue;
            const key = reverseDictionary.get(content);
            if (!key) continue;
            knownTextNodes.set(node, key);
            const translated = t(key, content);
            const start = original.indexOf(content);
            node.nodeValue = `${original.slice(0, start)}${translated}${original.slice(start + content.length)}`;
        }
        for (const element of root.querySelectorAll('[title], [placeholder], [aria-label]')) {
            const attributes = knownAttributes.get(element) || new Map();
            for (const attribute of ['title', 'placeholder', 'aria-label']) {
                if (!element.hasAttribute(attribute) || attributes.has(attribute)) continue;
                const value = element.getAttribute(attribute);
                const key = reverseDictionary.get(value);
                if (!key) continue;
                attributes.set(attribute, key);
                element.setAttribute(attribute, t(key, value));
            }
            if (attributes.size) knownAttributes.set(element, attributes);
        }
    }

    function refreshKnownText() {
        for (const [node, key] of knownTextNodes) {
            if (!node.isConnected) {
                knownTextNodes.delete(node);
                continue;
            }
            const original = node.nodeValue || '';
            const content = original.trim();
            const start = original.indexOf(content);
            node.nodeValue = `${original.slice(0, start)}${t(key, content)}${original.slice(start + content.length)}`;
        }
        for (const [element, attributes] of knownAttributes) {
            if (!element.isConnected) {
                knownAttributes.delete(element);
                continue;
            }
            for (const [attribute, key] of attributes) {
                element.setAttribute(attribute, t(key, element.getAttribute(attribute) || ''));
            }
        }
    }

    async function setLanguage(code, { refreshPage = false } = {}) {
        const normalized = normalizeLanguage(code);
        await loadDictionary(normalized);
        currentLanguage = normalized;
        localStorage.setItem(STORAGE_KEY, normalized);
        rebuildReverseDictionary();
        document.documentElement.lang = normalized;
        apply(document);
        refreshKnownText();
        window.dispatchEvent(new CustomEvent('deltamod-language-change', {
            detail: { language: normalized }
        }));
        if (refreshPage && typeof window.page === 'function' && window.pageN) {
            await window.page(window.pageN);
        }
        return normalized;
    }

    async function getLanguages() {
        await ready;
        return SUPPORTED_LANGUAGES.map(code => metadata.get(code));
    }

    const ready = initialize().catch(error => {
        console.error('Localization initialization failed:', error);
        currentLanguage = DEFAULT_LANGUAGE;
    });

    window.Localization = Object.freeze({
        ready,
        t,
        apply,
        applyKnownText,
        replaceTokens,
        translateKnownText,
        getLanguages,
        getLanguage: () => currentLanguage,
        setLanguage
    });
})();
