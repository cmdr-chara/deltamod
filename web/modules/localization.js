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
            community_options_subtitle: 'Configure Community without changing the official Deltamod profile.',
            optcat_data: 'Data',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Choose the language used by Deltamod Community. New Community features may fall back to English until their translations are updated.',
            language_current: 'Current language',
            language_select_hint: 'Select language',
            language_restart_note: 'The current page refreshes immediately after changing language.',
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
            community_alert_alignment: 'Alert alignment'
        },
        it: {
            community_options_subtitle: 'Configura Community senza modificare il profilo ufficiale di Deltamod.',
            optcat_data: 'Dati',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Scegli la lingua usata da Deltamod Community. Le nuove funzioni Community possono restare in inglese finché le traduzioni non vengono aggiornate.',
            language_current: 'Lingua attuale',
            language_select_hint: 'Seleziona lingua',
            language_restart_note: 'La pagina corrente si aggiorna subito dopo il cambio di lingua.',
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
            community_alert_alignment: 'Posizione degli avvisi'
        },
        pl: {
            community_options_subtitle: 'Skonfiguruj Community bez zmieniania oficjalnego profilu Deltamod.',
            optcat_data: 'Dane',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Wybierz język Deltamod Community. Nowe funkcje Community mogą pozostać po angielsku do czasu aktualizacji tłumaczeń.',
            language_current: 'Bieżący język',
            language_select_hint: 'Wybierz język',
            language_restart_note: 'Bieżąca strona odświeży się natychmiast po zmianie języka.',
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
            community_alert_alignment: 'Położenie powiadomień'
        },
        es: {
            community_options_subtitle: 'Configura Community sin modificar el perfil oficial de Deltamod.',
            optcat_data: 'Datos',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Elige el idioma de Deltamod Community. Las funciones nuevas pueden mostrarse en inglés hasta que se actualice su traducción.',
            language_current: 'Idioma actual',
            language_select_hint: 'Seleccionar idioma',
            language_restart_note: 'La página actual se actualiza inmediatamente al cambiar de idioma.',
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
            community_alert_alignment: 'Posición de los avisos'
        },
        fr: {
            community_options_subtitle: 'Configurez Community sans modifier le profil officiel de Deltamod.',
            optcat_data: 'Données',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Choisissez la langue de Deltamod Community. Les nouvelles fonctions peuvent rester en anglais jusqu’à leur traduction.',
            language_current: 'Langue actuelle',
            language_select_hint: 'Choisir la langue',
            language_restart_note: 'La page actuelle s’actualise immédiatement après le changement de langue.',
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
            community_alert_alignment: 'Position des alertes'
        },
        de: {
            community_options_subtitle: 'Community konfigurieren, ohne das offizielle Deltamod-Profil zu verändern.',
            optcat_data: 'Daten',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Wähle die Sprache von Deltamod Community. Neue Funktionen können bis zur Übersetzung auf Englisch angezeigt werden.',
            language_current: 'Aktuelle Sprache',
            language_select_hint: 'Sprache auswählen',
            language_restart_note: 'Die aktuelle Seite wird nach dem Sprachwechsel sofort neu geladen.',
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
            community_alert_alignment: 'Position der Hinweise'
        },
        'pt-br': {
            community_options_subtitle: 'Configure o Community sem alterar o perfil oficial do Deltamod.',
            optcat_data: 'Dados',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Escolha o idioma do Deltamod Community. Recursos novos podem permanecer em inglês até que a tradução seja atualizada.',
            language_current: 'Idioma atual',
            language_select_hint: 'Selecionar idioma',
            language_restart_note: 'A página atual é atualizada imediatamente após a troca de idioma.',
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
            community_alert_alignment: 'Posição dos alertas'
        },
        ja: {
            community_options_subtitle: '公式Deltamodのプロフィールを変更せずにCommunityを設定します。',
            optcat_data: 'データ',
            optcat_nexus: 'Nexus Mods',
            language_help: 'Deltamod Communityで使用する言語を選択します。新機能は翻訳されるまで英語で表示される場合があります。',
            language_current: '現在の言語',
            language_select_hint: '言語を選択',
            language_restart_note: '言語を変更すると、現在のページがすぐに更新されます。',
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
            community_alert_alignment: '通知の位置'
        }
    });

    let currentLanguage = normalizeLanguage(
        localStorage.getItem(STORAGE_KEY) || navigator.language
    );
    let reverseDictionary = new Map();

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
        const selected = dictionaries.get(currentLanguage) || english;
        for (const [key, englishValue] of Object.entries(english)) {
            if (typeof englishValue !== 'string' || englishValue.includes('{')) continue;
            const translated = selected[key];
            if (typeof translated === 'string' && translated !== englishValue) {
                reverseDictionary.set(englishValue, translated);
            }
        }
    }

    async function initialize() {
        await Promise.all([
            loadDictionary(DEFAULT_LANGUAGE),
            loadDictionary(currentLanguage),
            ...SUPPORTED_LANGUAGES.map(loadMetadata)
        ]);
        rebuildReverseDictionary();
        document.documentElement.lang = currentLanguage;
    }

    function t(key, fallback = key, ...args) {
        const selected = dictionaries.get(currentLanguage) || {};
        const english = dictionaries.get(DEFAULT_LANGUAGE) || {};
        return interpolate(selected[key] ?? english[key] ?? fallback, args);
    }

    function translateKnownText(value) {
        if (typeof value !== 'string') return value;
        return reverseDictionary.get(value) || value;
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

    async function setLanguage(code) {
        const normalized = normalizeLanguage(code);
        await loadDictionary(normalized);
        currentLanguage = normalized;
        localStorage.setItem(STORAGE_KEY, normalized);
        rebuildReverseDictionary();
        document.documentElement.lang = normalized;
        apply(document);
        window.dispatchEvent(new CustomEvent('deltamod-language-change', {
            detail: { language: normalized }
        }));
        if (typeof window.page === 'function' && window.pageN) {
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
        replaceTokens,
        translateKnownText,
        getLanguages,
        getLanguage: () => currentLanguage,
        setLanguage
    });
})();
