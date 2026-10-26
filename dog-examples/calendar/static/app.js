// ==========================================================================
// DogRS Calendar — Real-World Collaborative Calendar Application
// Iconography: Lucide Vector Set (https://icones.js.org/collection/lucide)
// Principles: Superside 13 Principles of Design (Repetition, Unity, Proportion, Harmony)
// ==========================================================================

const Icons = {
    video: (size = 12) => `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="lucide-icon"><path d="m16 13 5.223 3.482a.5.5 0 0 0 .777-.416V7.934a.5.5 0 0 0-.777-.416L16 11"/><rect width="14" height="12" x="2" y="6" rx="2"/></svg>`,
    repeat: (size = 11) => `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="lucide-icon"><path d="m17 2 4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/></svg>`,
    pin: (size = 12) => `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="lucide-icon"><path d="M20 10c0 4.993-5.539 10.193-7.399 11.799a1 1 0 0 1-1.202 0C9.539 20.193 4 14.993 4 10a8 8 0 0 1 16 0"/><circle cx="12" cy="10" r="3"/></svg>`,
    bell: (size = 13) => `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="lucide-icon"><path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/></svg>`,
    calendar: (size = 14) => `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="lucide-icon"><path d="M8 2v4"/><path d="M16 2v4"/><rect width="18" height="18" x="3" y="4" rx="2"/><path d="M3 10h18"/><path d="M8 14h.01"/><path d="M12 14h.01"/><path d="M16 14h.01"/><path d="M8 18h.01"/><path d="M12 18h.01"/><path d="M16 18h.01"/></svg>`,
    clock: (size = 12) => `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="lucide-icon"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>`,
    check: (size = 12) => `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" class="lucide-icon"><path d="M20 6 9 17l-5-5"/></svg>`,
};

class CalendarApp {
    constructor() {
        this.events = [];
        this.currentDate = new Date();
        this.currentView = 'week'; // 'day' | 'week' | 'month' | 'agenda'
        this.currentWorkspace = 'default';
        this.searchQuery = '';
        this.selectedEvent = null;
        this.editingEventId = null;
        this.ws = null;
        this.enabledColors = new Set(['indigo', 'emerald', 'amber', 'rose']);
        this.notifications = [];
        this.unreadNotificationsCount = 0;

        this.initElements();
        this.initEventListeners();
        this.initWebSocket();
        this.loadEvents();
    }

    initElements() {
        // Nav
        this.elMonthHeading = document.getElementById('current-month-heading');
        this.elBtnToday = document.getElementById('btn-today');
        this.elBtnPrev = document.getElementById('btn-prev');
        this.elBtnNext = document.getElementById('btn-next');
        this.elInputSearch = document.getElementById('input-search');
        this.elViewTabs = document.querySelectorAll('.view-tab');
        this.elSyncIndicator = document.getElementById('sync-indicator');
        this.elSyncLabel = document.getElementById('sync-label');
        this.elSelectWorkspace = document.getElementById('select-workspace');
        this.elBtnToggleDev = document.getElementById('btn-toggle-dev');
        this.elBtnNotifications = document.getElementById('btn-notifications');
        this.elNotificationsBadge = document.getElementById('notifications-badge');
        this.elNotificationsDropdown = document.getElementById('notifications-dropdown');
        this.elNotificationsList = document.getElementById('notifications-list');
        this.elBtnClearNotifications = document.getElementById('btn-clear-notifications');
        this.elHeaderBellIcon = document.getElementById('header-bell-icon');
        this.elBtnCreateEvent = document.getElementById('btn-create-event');

        // Sidebar & Mobile
        this.elSidebarRail = document.getElementById('sidebar-rail');
        this.elBtnToggleSidebar = document.getElementById('btn-toggle-sidebar');
        this.elBtnCloseSidebar = document.getElementById('btn-close-sidebar');
        this.elSidebarBackdrop = document.getElementById('sidebar-backdrop');
        this.elPopoverBackdrop = document.getElementById('popover-backdrop');
        this.elBtnMobileFab = document.getElementById('btn-mobile-fab');

        this.elMiniMonthLabel = document.getElementById('mini-month-label');
        this.elMiniMonthGrid = document.getElementById('mini-month-grid');
        this.elUpcomingCallout = document.getElementById('upcoming-callout');
        this.elNextMeetingTitle = document.getElementById('next-meeting-title');
        this.elNextMeetingTime = document.getElementById('next-meeting-time');
        this.elNextMeetingLink = document.getElementById('next-meeting-link');
        this.elRemindersList = document.getElementById('reminders-list');

        // Canvas
        this.elCanvasContent = document.getElementById('canvas-content');

        // Popover
        this.elPopover = document.getElementById('event-popover');
        this.elPopoverSwatch = document.getElementById('popover-swatch');
        this.elPopoverTitle = document.getElementById('popover-title');
        this.elPopoverTime = document.getElementById('popover-time');
        this.elPopoverJoinBtn = document.getElementById('popover-join-btn');
        this.elPopoverLocationRow = document.getElementById('popover-location-row');
        this.elPopoverLocation = document.getElementById('popover-location');
        this.elPopoverDescRow = document.getElementById('popover-desc-row');
        this.elPopoverDesc = document.getElementById('popover-desc');
        this.elPopoverReminderText = document.getElementById('popover-reminder-text');
        this.elPopoverRepeatRow = document.getElementById('popover-repeat-row');
        this.elPopoverRepeatText = document.getElementById('popover-repeat-text');
        this.elBtnPopoverClose = document.getElementById('btn-popover-close');
        this.elBtnPopoverEdit = document.getElementById('btn-popover-edit');
        this.elBtnPopoverDelete = document.getElementById('btn-popover-delete');

        // Modal
        this.elModal = document.getElementById('modal-event');
        this.elModalHeadingTitle = document.getElementById('modal-heading-title');
        this.elBtnSaveEvent = document.getElementById('btn-save-event');
        this.elForm = document.getElementById('form-event');
        this.elInputTitle = document.getElementById('input-title');
        this.elInputStart = document.getElementById('input-start');
        this.elInputEnd = document.getElementById('input-end');
        this.elInputMeeting = document.getElementById('input-meeting');
        this.elInputLocation = document.getElementById('input-location');
        this.elInputDesc = document.getElementById('input-desc');
        this.elBtnCloseModal = document.getElementById('btn-close-modal');
        this.elBtnCancelModal = document.getElementById('btn-cancel-modal');
        this.elBtnAddMeet = document.getElementById('btn-add-meet');
        this.elSelectRecurrence = document.getElementById('select-recurrence');
        this.elOptWeekly = document.getElementById('opt-weekly');
        this.elOptBiweekly = document.getElementById('opt-biweekly');
        this.elOptMonthly = document.getElementById('opt-monthly');
        this.elOptCustom = document.getElementById('opt-custom');
        this.elCustomRecPanel = document.getElementById('custom-recurrence-panel');
        this.elInputCustomInterval = document.getElementById('input-custom-interval');
        this.elSelectCustomFreq = document.getElementById('select-custom-freq');
        this.elCustomWeekdaysGroup = document.getElementById('custom-weekdays-group');
        this.elWeekdayChips = document.querySelectorAll('.btn-weekday');
        this.elCustomEndsRadios = document.querySelectorAll('input[name="custom-ends"]');
        this.elInputCustomUntil = document.getElementById('input-custom-until');
        this.elInputCustomCount = document.getElementById('input-custom-count');
        this.elCustomSummaryText = document.getElementById('custom-summary-text');

        // Dev Inspector
        this.elDevInspector = document.getElementById('dev-inspector');
        this.elInspectorFeed = document.getElementById('inspector-feed');
        this.elBtnClearInspector = document.getElementById('btn-clear-inspector');
        this.elBtnCloseInspector = document.getElementById('btn-close-inspector');

        // Toasts
        this.elToastShelf = document.getElementById('toast-shelf');
    }

    initEventListeners() {
        // Date Navigation
        this.elBtnToday.addEventListener('click', () => {
            this.currentDate = new Date();
            this.render();
        });

        this.elBtnPrev.addEventListener('click', () => this.navigateDate(-1));
        this.elBtnNext.addEventListener('click', () => this.navigateDate(1));

        // View Tabs
        this.elViewTabs.forEach(tab => {
            tab.addEventListener('click', () => {
                this.elViewTabs.forEach(t => t.classList.remove('active'));
                tab.classList.add('active');
                this.currentView = tab.dataset.view;
                if (window.innerWidth < 768) this.closeSidebar();
                this.render();
            });
        });

        // Mobile Controls
        this.elBtnToggleSidebar?.addEventListener('click', () => this.toggleSidebar());
        this.elBtnCloseSidebar?.addEventListener('click', () => this.closeSidebar());
        this.elSidebarBackdrop?.addEventListener('click', () => this.closeSidebar());
        this.elPopoverBackdrop?.addEventListener('click', () => this.closePopover());
        this.elBtnMobileFab?.addEventListener('click', () => this.openCreateModal());

        // Search Input
        this.elInputSearch.addEventListener('input', (e) => {
            this.searchQuery = e.target.value.toLowerCase().trim();
            this.render();
        });

        // Workspace Selector
        this.elSelectWorkspace.addEventListener('change', (e) => {
            this.currentWorkspace = e.target.value;
            this.logDev('WORKSPACE', `Switched workspace to [${this.currentWorkspace}]`);
            this.loadEvents();
        });

        // Dev Inspector Toggle
        this.elBtnToggleDev?.addEventListener('click', () => {
            this.elDevInspector?.classList.toggle('hidden');
        });
        this.elBtnCloseInspector?.addEventListener('click', () => {
            this.elDevInspector?.classList.add('hidden');
        });
        this.elBtnClearInspector?.addEventListener('click', () => {
            if (this.elInspectorFeed) this.elInspectorFeed.innerHTML = '';
        });

        // Notifications Dropdown Toggle
        if (this.elBtnNotifications) {
            this.elBtnNotifications.addEventListener('click', (e) => {
                e.stopPropagation();
                const isOpen = !this.elNotificationsDropdown.classList.contains('hidden');
                if (isOpen) {
                    this.elNotificationsDropdown.classList.add('hidden');
                } else {
                    this.elNotificationsDropdown.classList.remove('hidden');
                    this.markNotificationsAsRead();
                    if (typeof Notification !== 'undefined' && Notification.permission === 'default') {
                        Notification.requestPermission();
                    }
                }
            });
        }

        if (this.elBtnClearNotifications) {
            this.elBtnClearNotifications.addEventListener('click', (e) => {
                e.stopPropagation();
                this.notifications = [];
                this.renderNotificationsList();
            });
        }

        // Close notifications dropdown on outside click
        document.addEventListener('click', (e) => {
            if (this.elNotificationsDropdown && !this.elNotificationsDropdown.classList.contains('hidden')) {
                if (!this.elNotificationsDropdown.contains(e.target) && !this.elBtnNotifications.contains(e.target)) {
                    this.elNotificationsDropdown.classList.add('hidden');
                }
            }
        });

        // Color Layer Checkboxes
        document.querySelectorAll('.layer-check').forEach(check => {
            check.addEventListener('change', () => {
                const color = check.dataset.color;
                if (check.checked) {
                    this.enabledColors.add(color);
                } else {
                    this.enabledColors.delete(color);
                }
                this.render();
            });
        });

        // Modal Open / Close
        this.elBtnCreateEvent.addEventListener('click', () => this.openCreateModal());
        this.elBtnCloseModal.addEventListener('click', () => this.elModal.close());
        this.elBtnCancelModal.addEventListener('click', () => this.elModal.close());
        this.elModal.addEventListener('close', () => {
            this.editingEventId = null;
        });

        // Light dismiss on backdrop click
        this.elModal.addEventListener('click', (e) => {
            const rect = this.elModal.getBoundingClientRect();
            const isInDialog = (
                rect.top <= e.clientY && e.clientY <= rect.top + rect.height &&
                rect.left <= e.clientX && e.clientX <= rect.left + rect.width
            );
            if (!isInDialog) this.elModal.close();
        });

        // Duration Presets
        document.querySelectorAll('.btn-duration').forEach(btn => {
            btn.addEventListener('click', () => {
                document.querySelectorAll('.btn-duration').forEach(b => b.classList.remove('active'));
                btn.classList.add('active');
                const min = parseInt(btn.dataset.min, 10);
                this.applyDuration(min);
            });
        });

        // Add Google Meet CTA
        if (this.elBtnAddMeet) {
            this.elBtnAddMeet.addEventListener('click', () => {
                this.elInputMeeting.value = 'https://meet.google.com/new';
                this.showToast('Google Meet Added', 'Meeting URL set to https://meet.google.com/new');
            });
        }

        // Form Submit
        this.elInputStart.addEventListener('change', () => {
            this.updateRecurrenceLabels();
            if (this.elSelectRecurrence && this.elSelectRecurrence.value === 'custom') {
                this.syncWeekdayChipsWithStartDate();
                this.updateCustomRecurrenceSummary();
            }
        });

        // Custom Recurrence Listeners
        if (this.elSelectRecurrence) {
            this.elSelectRecurrence.addEventListener('change', () => {
                const isCustom = this.elSelectRecurrence.value === 'custom';
                if (this.elCustomRecPanel) {
                    this.elCustomRecPanel.classList.toggle('hidden', !isCustom);
                    if (isCustom) {
                        this.syncWeekdayChipsWithStartDate();
                        this.updateCustomRecurrenceSummary();
                    }
                }
            });
        }

        if (this.elSelectCustomFreq) {
            this.elSelectCustomFreq.addEventListener('change', () => {
                const isWeek = this.elSelectCustomFreq.value === 'week';
                if (this.elCustomWeekdaysGroup) {
                    this.elCustomWeekdaysGroup.style.display = isWeek ? 'flex' : 'none';
                }
                this.updateCustomRecurrenceSummary();
            });
        }

        if (this.elInputCustomInterval) {
            this.elInputCustomInterval.addEventListener('input', () => this.updateCustomRecurrenceSummary());
        }

        if (this.elWeekdayChips) {
            this.elWeekdayChips.forEach(chip => {
                chip.addEventListener('click', () => {
                    const activeCount = Array.from(this.elWeekdayChips).filter(c => c.classList.contains('active')).length;
                    if (chip.classList.contains('active') && activeCount <= 1) {
                        return; // Prevent deselecting last active day
                    }
                    chip.classList.toggle('active');
                    this.updateCustomRecurrenceSummary();
                });
            });
        }

        if (this.elCustomEndsRadios) {
            this.elCustomEndsRadios.forEach(radio => {
                radio.addEventListener('change', () => {
                    const mode = radio.value;
                    if (this.elInputCustomUntil) {
                        this.elInputCustomUntil.disabled = (mode !== 'until');
                        if (mode === 'until' && !this.elInputCustomUntil.value) {
                            const u = new Date();
                            u.setMonth(u.getMonth() + 3);
                            this.elInputCustomUntil.value = this.formatDateKey(u);
                        }
                    }
                    if (this.elInputCustomCount) {
                        this.elInputCustomCount.disabled = (mode !== 'count');
                    }
                    this.updateCustomRecurrenceSummary();
                });
            });
        }

        if (this.elInputCustomUntil) {
            this.elInputCustomUntil.addEventListener('input', () => this.updateCustomRecurrenceSummary());
        }
        if (this.elInputCustomCount) {
            this.elInputCustomCount.addEventListener('input', () => this.updateCustomRecurrenceSummary());
        }

        this.elForm.addEventListener('submit', (e) => {
            e.preventDefault();
            this.saveNewEvent();
        });
        this.elForm.addEventListener('keydown', (e) => {
            if (e.key === 'Enter' && e.target.tagName !== 'TEXTAREA') {
                e.preventDefault();
                if (this.elForm.checkValidity && !this.elForm.checkValidity()) {
                    this.elForm.reportValidity();
                    return;
                }
                this.saveNewEvent();
            }
        });
        this.elBtnSaveEvent?.addEventListener('click', (e) => {
            if (this.elForm.checkValidity && !this.elForm.checkValidity()) {
                this.elForm.reportValidity();
                return;
            }
            e.preventDefault();
            this.saveNewEvent();
        });

        // Popover Actions
        this.elBtnPopoverClose.addEventListener('click', () => this.closePopover());
        this.elBtnPopoverEdit?.addEventListener('click', () => {
            if (this.selectedEvent) {
                this.openEditModal(this.selectedEvent);
            }
        });
        this.elBtnPopoverDelete.addEventListener('click', () => {
            if (this.selectedEvent) {
                this.deleteEvent(this.selectedEvent.id);
            }
        });

        // Global Keyboard Shortcuts
        window.addEventListener('keydown', (e) => {
            if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA' || e.target.tagName === 'SELECT') {
                if (e.key === 'Escape') {
                    if (this.elModal.open) this.elModal.close();
                    this.closePopover();
                }
                return;
            }

            if (e.key === 'c' || e.key === 'C') {
                e.preventDefault();
                this.openCreateModal();
            } else if ((e.key === 'e' || e.key === 'E') && this.selectedEvent && !this.elPopover.classList.contains('hidden')) {
                e.preventDefault();
                this.openEditModal(this.selectedEvent);
            } else if (e.key === 't' || e.key === 'T') {
                e.preventDefault();
                this.currentDate = new Date();
                this.render();
            } else if (e.key === '/' || (e.metaKey && e.key === 'k') || (e.ctrlKey && e.key === 'k')) {
                e.preventDefault();
                this.elInputSearch.focus();
            } else if (e.key === '1') {
                this.switchView('day');
            } else if (e.key === '2') {
                this.switchView('week');
            } else if (e.key === '3') {
                this.switchView('month');
            } else if (e.key === '4') {
                this.switchView('agenda');
            } else if (e.key === 'Escape') {
                this.closePopover();
                if (this.elModal.open) this.elModal.close();
            }
        });
    }

    switchView(viewName) {
        this.currentView = viewName;
        this.elViewTabs.forEach(t => t.classList.toggle('active', t.dataset.view === viewName));
        this.render();
    }

    navigateDate(direction) {
        if (this.currentView === 'day') {
            this.currentDate.setDate(this.currentDate.getDate() + direction);
        } else if (this.currentView === 'week') {
            this.currentDate.setDate(this.currentDate.getDate() + (direction * 7));
        } else if (this.currentView === 'month') {
            this.currentDate.setMonth(this.currentDate.getMonth() + direction);
        } else {
            this.currentDate.setDate(this.currentDate.getDate() + (direction * 7));
        }
        this.render();
    }

    // =========================================================================
    // REST API Integration & Realistic Starter Seeding
    // =========================================================================
    async loadEvents() {
        try {
            const res = await fetch('/api/events', {
                headers: { 'x-tenant-id': this.currentWorkspace }
            });
            if (!res.ok) throw new Error(`HTTP ${res.status}`);

            this.events = await res.json();

            // If empty in default workspace, seed realistic calendar schedule
            if (this.events.length === 0 && this.currentWorkspace === 'default') {
                await this.seedRealisticCalendar();
            } else {
                this.render();
            }
        } catch (err) {
            this.logDev('ERROR', `Failed to load events: ${err.message}`);
        }
    }

    async seedRealisticCalendar() {
        const today = new Date();
        const y = today.getFullYear();
        const m = today.getMonth();
        const d = today.getDate();

        const starters = [
            {
                title: 'Daily Engineering Standup',
                description: 'Sync on sprint backlog, blockers, and DogRS queue architecture.',
                start_time: new Date(y, m, d, 9, 30).toISOString(),
                end_time: new Date(y, m, d, 10, 0).toISOString(),
                location: 'Remote',
                meeting_url: 'https://meet.google.com/new',
                color: 'indigo',
                recurrence: 'weekdays'
            },
            {
                title: 'Product & Architecture Review',
                description: 'Deep dive into real-time event streaming and cancel-wins guarantees.',
                start_time: new Date(y, m, d, 14, 0).toISOString(),
                end_time: new Date(y, m, d, 15, 0).toISOString(),
                location: 'Conference Room 4B',
                meeting_url: 'https://meet.google.com/new',
                color: 'emerald',
                recurrence: 'weekly'
            },
            {
                title: '1:1 with Engineering Lead',
                description: 'Bi-weekly career growth, performance feedback, and technical mentorship.',
                start_time: new Date(y, m, d + 1, 11, 0).toISOString(),
                end_time: new Date(y, m, d + 1, 11, 45).toISOString(),
                location: 'Zoom Meeting',
                meeting_url: 'https://zoom.us/j/9876543210',
                color: 'rose',
                recurrence: 'biweekly'
            },
            {
                title: 'Sprint Retrospective & Demo',
                description: 'Team demo of real-time scheduling UI and delayed reminder notifications.',
                start_time: new Date(y, m, d + 2, 16, 0).toISOString(),
                end_time: new Date(y, m, d + 2, 17, 0).toISOString(),
                location: 'Main Auditorium & Live Stream',
                meeting_url: 'https://meet.google.com/new',
                color: 'amber',
                recurrence: 'biweekly'
            }
        ];

        for (const item of starters) {
            try {
                const res = await fetch('/api/events', {
                    method: 'POST',
                    headers: {
                        'Content-Type': 'application/json',
                        'x-tenant-id': 'default'
                    },
                    body: JSON.stringify(item)
                });
                if (res.ok) {
                    const saved = await res.json();
                    if (!this.events.some(x => x.id === saved.id)) {
                        this.events.push(saved);
                    }
                }
            } catch (err) {
                console.error('Seeding error', err);
            }
        }

        this.render();
    }

    async saveNewEvent() {
        const title = this.elInputTitle.value.trim();
        const start = this.elInputStart.value;
        const end = this.elInputEnd.value;
        const meeting_url = this.elInputMeeting.value.trim();
        const location = this.elInputLocation.value.trim();
        const description = this.elInputDesc.value.trim();
        const colorRadio = document.querySelector('input[name="color"]:checked');
        const color = colorRadio ? colorRadio.value : 'indigo';

        const recVal = this.elSelectRecurrence ? this.elSelectRecurrence.value : 'none';
        const recurrence = (recVal === 'custom') ? this.buildCustomRecurrenceString() : recVal;

        if (!title || !start) {
            if (this.elForm.reportValidity) this.elForm.reportValidity();
            return;
        }

        const isEdit = !!this.editingEventId;
        const payload = {
            title,
            start_time: new Date(start).toISOString(),
            end_time: end ? new Date(end).toISOString() : null,
            recurrence: recurrence || 'none',
            meeting_url: meeting_url || '',
            location: location || '',
            description: description || '',
            color
        };

        const url = isEdit ? `/api/events/${this.editingEventId}` : '/api/events';
        const method = isEdit ? 'PATCH' : 'POST';

        try {
            this.logDev('REST', `${method} ${url}: "${title}"`);
            const res = await fetch(url, {
                method,
                headers: {
                    'Content-Type': 'application/json',
                    'x-tenant-id': this.currentWorkspace
                },
                body: JSON.stringify(payload)
            });

            if (!res.ok) {
                const err = await res.json().catch(() => ({ message: `HTTP ${res.status}` }));
                throw new Error(err.message || `HTTP ${res.status}`);
            }

            const result = await res.json();
            const wasEditingId = this.editingEventId;
            this.editingEventId = null;
            this.elModal.close();
            this.elForm.reset();

            if (isEdit) {
                const idx = this.events.findIndex(e => e.id === wasEditingId);
                if (idx !== -1) {
                    this.events[idx] = { ...this.events[idx], ...result };
                } else {
                    this.events.push(result);
                }
                this.showToast('Event Updated', title, 'created');
            } else {
                if (!this.events.some(e => e.id === result.id)) {
                    this.events.push(result);
                }
                this.showToast('Event Scheduled', title, 'created');
            }

            this.render();
        } catch (err) {
            console.error('Failed to save event:', err);
            this.logDev('ERROR', `Save event failed: ${err.message}`);
            this.showToast('Error', err.message, 'error');
        }
    }

    async deleteEvent(id) {
        try {
            this.logDev('REST', `DELETE /api/events/${id}`);
            const res = await fetch(`/api/events/${id}`, {
                method: 'DELETE',
                headers: { 'x-tenant-id': this.currentWorkspace }
            });

            if (!res.ok) throw new Error(`HTTP ${res.status}`);

            this.closePopover();
            this.events = this.events.filter(e => e.id !== id);
            this.render();

            this.showToast('Event Deleted', 'Pending reminder canceled (cancel-wins)', 'reminder');
        } catch (err) {
            this.logDev('ERR', `Delete failed: ${err.message}`);
        }
    }

    // =========================================================================
    // WebSocket Real-Time Bus
    // =========================================================================
    initWebSocket() {
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host || '127.0.0.1:3036'}/ws`;

        this.ws = new WebSocket(wsUrl);

        this.ws.onopen = () => {
            this.elSyncIndicator.classList.remove('offline');
            this.elSyncLabel.textContent = 'Synced';
            this.logDev('WS', 'Connected to real-time event bus (broadcast channel: event_channel)');
        };

        this.ws.onclose = () => {
            this.elSyncIndicator.classList.add('offline');
            this.elSyncLabel.textContent = 'Reconnecting...';
            setTimeout(() => this.initWebSocket(), 2500);
        };

        this.ws.onmessage = (e) => {
            try {
                const msg = JSON.parse(e.data);
                this.handleWebSocketBroadcast(msg);
            } catch (err) {
                console.error('WS Parse Error', err);
            }
        };
    }

    handleWebSocketBroadcast(msg) {
        if (msg.type === 'BROADCAST') {
            const { event, payload } = msg;

            // Tenant isolation: ignore broadcasts intended for other workspaces
            if (payload && payload.tenant && payload.tenant !== this.currentWorkspace) {
                return;
            }
            if (msg.tenant && msg.tenant !== this.currentWorkspace) {
                return;
            }

            this.logDev('WS-PACKET', `[${event}] ${JSON.stringify(payload)}`);

            if (event === 'reminder') {
                this.handleReminderNotification(payload);
                return;
            }

            if (event === 'created') {
                if (!this.events.some(x => x.id === payload.id)) {
                    this.events.push(payload);
                    this.showToast('New Event Added', payload.title, 'created');
                }
            } else if (event === 'updated' || event === 'patched') {
                const idx = this.events.findIndex(x => x.id === payload.id);
                if (idx !== -1) {
                    this.events[idx] = { ...this.events[idx], ...payload };
                } else {
                    this.events.push(payload);
                }
            } else if (event === 'removed') {
                this.events = this.events.filter(x => x.id !== payload.id);
            }

            this.render();
        }
    }

    // =========================================================================
    // Rendering Core
    // =========================================================================
    render() {
        this.renderTopHeading();
        this.renderMiniCalendar();
        this.renderUpcomingCallout();
        this.renderSmartReminders();

        if (this.currentView === 'day') {
            this.renderDayView();
        } else if (this.currentView === 'week') {
            this.renderWeekView();
        } else if (this.currentView === 'month') {
            this.renderMonthView();
        } else {
            this.renderAgendaView();
        }
    }

    getFilteredEvents() {
        return this.events.filter(e => {
            // Color layer filter
            const color = e.color || 'indigo';
            if (!this.enabledColors.has(color)) return false;

            // Search query filter
            if (this.searchQuery) {
                const text = `${e.title || ''} ${e.description || ''} ${e.location || ''}`.toLowerCase();
                if (!text.includes(this.searchQuery)) return false;
            }
            return true;
        });
    }

    getEventsForRange(rangeStart, rangeEnd) {
        const base = this.getFilteredEvents();
        return this.expandOccurrences(base, rangeStart, rangeEnd);
    }

    expandOccurrences(events, rangeStart, rangeEnd) {
        return this.expandEventsForRange(events, rangeStart, rangeEnd);
    }

    parseRecurrenceRule(recurrence, origStart) {
        if (!recurrence || recurrence === 'none') return null;
        if (recurrence === 'daily') {
            return { freq: 'day', interval: 1, days: null, ends: 'never', until: null, count: null };
        }
        if (recurrence === 'weekdays') {
            return { freq: 'week', interval: 1, days: [1, 2, 3, 4, 5], ends: 'never', until: null, count: null };
        }
        if (recurrence === 'weekly') {
            return { freq: 'week', interval: 1, days: [origStart.getDay()], ends: 'never', until: null, count: null };
        }
        if (recurrence === 'biweekly') {
            return { freq: 'week', interval: 2, days: [origStart.getDay()], ends: 'never', until: null, count: null };
        }
        if (recurrence === 'monthly') {
            return { freq: 'month', interval: 1, days: null, ends: 'never', until: null, count: null };
        }
        if (recurrence === 'yearly') {
            return { freq: 'year', interval: 1, days: null, ends: 'never', until: null, count: null };
        }
        if (recurrence.startsWith('custom:')) {
            const rule = {
                freq: 'week',
                interval: 1,
                days: [origStart.getDay()],
                ends: 'never',
                until: null,
                count: null
            };
            const parts = recurrence.slice(7).split(';');
            for (const part of parts) {
                const [k, v] = part.split('=');
                if (k === 'freq') rule.freq = v;
                else if (k === 'interval') rule.interval = Math.max(1, parseInt(v, 10) || 1);
                else if (k === 'days') rule.days = v ? v.split(',').map(Number) : [origStart.getDay()];
                else if (k === 'ends') rule.ends = v;
                else if (k === 'until') rule.until = v;
                else if (k === 'count') rule.count = Math.max(1, parseInt(v, 10) || 1);
            }
            return rule;
        }
        return null;
    }

    expandEventsForRange(events, rangeStart, rangeEnd) {
        const startTs = rangeStart.getTime();
        const endTs = rangeEnd.getTime();
        const result = [];

        // Deduplicate input events
        const seenIds = new Set();
        const uniqueEvents = [];
        for (const e of events) {
            if (e && e.id && !seenIds.has(e.id)) {
                seenIds.add(e.id);
                uniqueEvents.push(e);
            }
        }

        for (const evt of uniqueEvents) {
            if (!evt.start_time) {
                result.push(evt);
                continue;
            }

            const origStart = new Date(evt.start_time);
            if (isNaN(origStart.getTime())) {
                result.push(evt);
                continue;
            }

            const recurrence = evt.recurrence || 'none';
            const rule = this.parseRecurrenceRule(recurrence, origStart);
            if (!rule) {
                result.push(evt);
                continue;
            }

            const origEnd = evt.end_time ? new Date(evt.end_time) : new Date(origStart.getTime() + 3600000);
            const durationMs = Math.max(0, origEnd.getTime() - origStart.getTime());

            let untilTs = Infinity;
            if (rule.ends === 'until' && rule.until) {
                const uDate = new Date(rule.until + 'T23:59:59');
                if (!isNaN(uDate.getTime())) untilTs = uDate.getTime();
            }

            const maxCount = (rule.ends === 'count' && rule.count) ? rule.count : Infinity;
            let occurrenceIndex = 0;
            let safetyCounter = 0;
            const maxOccurrences = 250;

            if (rule.freq === 'week' && rule.days && rule.days.length > 0) {
                // Weekly recurrence with selected days
                let weekAnchor = new Date(origStart);
                weekAnchor.setDate(weekAnchor.getDate() - weekAnchor.getDay()); // Sunday of origStart week
                weekAnchor.setHours(origStart.getHours(), origStart.getMinutes(), origStart.getSeconds(), origStart.getMilliseconds());

                // Fast-forward weekAnchor if rangeStart is far ahead (only when not count-bounded)
                if (rule.ends !== 'count' && weekAnchor.getTime() < startTs - 7 * 86400000) {
                    const weeksDiff = Math.floor((startTs - weekAnchor.getTime()) / (7 * 86400000 * rule.interval));
                    if (weeksDiff > 1) {
                        weekAnchor.setDate(weekAnchor.getDate() + (weeksDiff - 1) * 7 * rule.interval);
                    }
                }

                while (weekAnchor.getTime() <= endTs + 7 * 86400000 && safetyCounter < maxOccurrences) {
                    safetyCounter++;
                    for (let dayOffset = 0; dayOffset < 7; dayOffset++) {
                        const dayCandidate = new Date(weekAnchor);
                        dayCandidate.setDate(dayCandidate.getDate() + dayOffset);

                        if (rule.days.includes(dayCandidate.getDay()) && dayCandidate.getTime() >= origStart.getTime()) {
                            if (dayCandidate.getTime() > untilTs || occurrenceIndex >= maxCount) {
                                break;
                            }
                            occurrenceIndex++;
                            const curEnd = new Date(dayCandidate.getTime() + durationMs);
                            if (dayCandidate.getTime() <= endTs && curEnd.getTime() >= startTs) {
                                result.push({
                                    ...evt,
                                    id: evt.id,
                                    _occurrenceDate: this.formatDateKey(dayCandidate),
                                    _isOccurrence: dayCandidate.getTime() !== origStart.getTime(),
                                    start_time: dayCandidate.toISOString(),
                                    end_time: curEnd.toISOString()
                                });
                            }
                        }
                    }

                    if (occurrenceIndex >= maxCount || weekAnchor.getTime() > untilTs) break;
                    weekAnchor.setDate(weekAnchor.getDate() + 7 * rule.interval);
                }
            } else {
                // Day, Month, or Year steps
                let cur = new Date(origStart);

                // Fast-forward for daily if far behind (only when not count-bounded)
                if (rule.freq === 'day' && rule.ends !== 'count' && cur.getTime() < startTs) {
                    const daysDiff = Math.floor((startTs - cur.getTime()) / (86400000 * rule.interval));
                    if (daysDiff > 1) {
                        cur.setDate(cur.getDate() + (daysDiff - 1) * rule.interval);
                    }
                }

                while (cur.getTime() <= endTs && safetyCounter < maxOccurrences) {
                    safetyCounter++;
                    if (cur.getTime() > untilTs || occurrenceIndex >= maxCount) {
                        break;
                    }
                    occurrenceIndex++;

                    const curEnd = new Date(cur.getTime() + durationMs);
                    if (cur.getTime() <= endTs && curEnd.getTime() >= startTs) {
                        result.push({
                            ...evt,
                            id: evt.id,
                            _occurrenceDate: this.formatDateKey(cur),
                            _isOccurrence: cur.getTime() !== origStart.getTime(),
                            start_time: cur.toISOString(),
                            end_time: curEnd.toISOString()
                        });
                    }

                    if (rule.freq === 'day') {
                        cur.setDate(cur.getDate() + rule.interval);
                    } else if (rule.freq === 'month') {
                        cur.setMonth(cur.getMonth() + rule.interval);
                    } else if (rule.freq === 'year') {
                        cur.setFullYear(cur.getFullYear() + rule.interval);
                    } else {
                        break;
                    }
                }
            }
        }

        return result;
    }

    formatRecurrence(recurrence, startTime) {
        if (!recurrence || recurrence === 'none') return 'Does not repeat';
        if (recurrence === 'daily') return 'Daily';
        if (recurrence === 'weekdays') return 'Every weekday (Mon - Fri)';
        if (recurrence === 'weekly') {
            if (startTime) {
                const day = new Date(startTime).toLocaleDateString(undefined, { weekday: 'long' });
                return `Weekly on ${day}`;
            }
            return 'Weekly';
        }
        if (recurrence === 'biweekly') {
            if (startTime) {
                const day = new Date(startTime).toLocaleDateString(undefined, { weekday: 'long' });
                return `Every 2 weeks on ${day}`;
            }
            return 'Every 2 weeks';
        }
        if (recurrence === 'monthly') {
            if (startTime) {
                const d = new Date(startTime).getDate();
                return `Monthly on day ${d}`;
            }
            return 'Monthly';
        }
        if (recurrence === 'yearly') return 'Annually';
        if (recurrence.startsWith('custom:')) {
            const rule = this.parseRecurrenceRule(recurrence, startTime ? new Date(startTime) : new Date());
            if (!rule) return 'Custom recurrence';
            const dayNames = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
            let freqStr = '';
            if (rule.interval === 1) {
                if (rule.freq === 'day') freqStr = 'Daily';
                else if (rule.freq === 'week') freqStr = 'Weekly';
                else if (rule.freq === 'month') freqStr = 'Monthly';
                else if (rule.freq === 'year') freqStr = 'Annually';
            } else {
                freqStr = `Every ${rule.interval} ${rule.freq}s`;
            }
            let daysStr = '';
            if (rule.freq === 'week' && rule.days && rule.days.length > 0) {
                if (rule.days.length === 5 && rule.days.every(d => d >= 1 && d <= 5)) {
                    daysStr = ' on weekdays';
                } else if (rule.days.length === 7) {
                    daysStr = ' on all days';
                } else {
                    daysStr = ' on ' + rule.days.map(d => dayNames[d]).join(', ');
                }
            }
            let endsStr = '';
            if (rule.ends === 'until' && rule.until) {
                endsStr = ` (until ${rule.until})`;
            } else if (rule.ends === 'count' && rule.count) {
                endsStr = ` (${rule.count} times)`;
            }
            return `${freqStr}${daysStr}${endsStr}`;
        }
        return recurrence;
    }

    renderTopHeading() {
        const monthName = this.currentDate.toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
        this.elMonthHeading.textContent = monthName;
    }

    // --- WEEK VIEW ---
    renderWeekView() {
        const weekDays = this.getWeekDays(this.currentDate);
        const todayStr = new Date().toDateString();
        const startOfRange = new Date(weekDays[0]);
        startOfRange.setHours(0, 0, 0, 0);
        const endOfRange = new Date(weekDays[6]);
        endOfRange.setHours(23, 59, 59, 999);
        const filtered = this.getEventsForRange(startOfRange, endOfRange);

        let html = `
            <div class="week-canvas">
                <div class="week-head-row">
                    <div class="gutter-hdr"></div>
                    ${weekDays.map(d => {
                        const isToday = d.toDateString() === todayStr;
                        return `
                            <div class="col-hdr ${isToday ? 'today' : ''}">
                                <span class="col-hdr-day">${d.toLocaleDateString(undefined, { weekday: 'short' })}</span>
                                <span class="col-hdr-date">${d.getDate()}</span>
                            </div>
                        `;
                    }).join('')}
                </div>
                <div class="week-grid-body">
                    <div class="time-gutter">
                        ${Array.from({ length: 13 }, (_, i) => i + 8).map(hour => {
                            const label = hour === 12 ? '12 PM' : (hour < 12 ? `${hour} AM` : `${hour - 12} PM`);
                            return `<div class="time-slot-label">${label}</div>`;
                        }).join('')}
                    </div>
                    ${weekDays.map(d => {
                        const isToday = d.toDateString() === todayStr;
                        const dateKey = this.formatDateKey(d);
                        return `
                            <div class="grid-day-col ${isToday ? 'today' : ''}" data-date="${dateKey}">
                                ${Array.from({ length: 13 }, (_, i) => i + 8).map(hour => `
                                    <div class="grid-hour-cell" data-date="${dateKey}" data-hour="${hour}" title="Click to schedule at ${hour}:00"></div>
                                `).join('')}
                                ${isToday ? `<div class="current-time-indicator" style="top: ${this.getCurrentTimePercent()}%"></div>` : ''}
                            </div>
                        `;
                    }).join('')}
                </div>
            </div>
        `;

        this.elCanvasContent.innerHTML = html;

        // Slot click for creation
        this.elCanvasContent.querySelectorAll('.grid-hour-cell').forEach(slot => {
            slot.addEventListener('click', () => {
                this.openCreateModalWithSlot(slot.dataset.date, parseInt(slot.dataset.hour, 10));
            });
        });

        // Event Tiles placement
        filtered.forEach(evt => {
            if (!evt.start_time) return;
            const startDate = new Date(evt.start_time);
            const dateKey = this.formatDateKey(startDate);
            const col = this.elCanvasContent.querySelector(`.grid-day-col[data-date="${dateKey}"]`);
            if (!col) return;

            const hourFrac = startDate.getHours() + (startDate.getMinutes() / 60);
            if (hourFrac < 8 || hourFrac > 21) return;

            const topPx = (hourFrac - 8) * 60;
            let duration = 1;
            if (evt.end_time) {
                duration = Math.max(0.4, (new Date(evt.end_time) - startDate) / (1000 * 60 * 60));
            }
            const heightPx = Math.max(28, duration * 60 - 3);

            const colorClass = `color-${this.sanitizeColor(evt.color)}`;

            const tile = document.createElement('div');
            tile.className = `event-tile ${colorClass}`;
            tile.style.top = `${topPx}px`;
            tile.style.height = `${heightPx}px`;

            tile.innerHTML = `
                <div class="event-tile-title">${this.escapeHtml(evt.title)}</div>
                <div class="event-tile-meta">
                    <span>${this.formatTime(startDate)}</span>
                    ${evt.recurrence && evt.recurrence !== 'none' ? `<span class="event-tile-repeat-badge" title="Repeats: ${this.escapeHtml(this.formatRecurrence(evt.recurrence, evt.start_time))}">${Icons.repeat(10)}</span>` : ''}
                    ${this.sanitizeUrl(evt.meeting_url) ? `<span class="event-tile-meet-badge">${Icons.video(11)}<span>Call</span></span>` : ''}
                </div>
            `;

            tile.addEventListener('click', (e) => {
                e.stopPropagation();
                this.openPopover(evt, tile);
            });
            tile.addEventListener('dblclick', (e) => {
                e.stopPropagation();
                this.openEditModal(evt);
            });

            col.appendChild(tile);
        });
    }

    // --- DAY VIEW ---
    renderDayView() {
        const todayStr = new Date().toDateString();
        const isToday = this.currentDate.toDateString() === todayStr;
        const dateKey = this.formatDateKey(this.currentDate);
        const startOfRange = new Date(this.currentDate);
        startOfRange.setHours(0, 0, 0, 0);
        const endOfRange = new Date(this.currentDate);
        endOfRange.setHours(23, 59, 59, 999);
        const filtered = this.getEventsForRange(startOfRange, endOfRange);

        const html = `
            <div class="day-view-canvas">
                <div class="time-gutter">
                    ${Array.from({ length: 13 }, (_, i) => i + 8).map(hour => {
                        const label = hour === 12 ? '12 PM' : (hour < 12 ? `${hour} AM` : `${hour - 12} PM`);
                        return `<div class="time-slot-label">${label}</div>`;
                    }).join('')}
                </div>
                <div class="grid-day-col ${isToday ? 'today' : ''}" data-date="${dateKey}">
                    ${Array.from({ length: 13 }, (_, i) => i + 8).map(hour => `
                        <div class="grid-hour-cell" data-date="${dateKey}" data-hour="${hour}" title="Click to schedule at ${hour}:00"></div>
                    `).join('')}
                    ${isToday ? `<div class="current-time-indicator" style="top: ${this.getCurrentTimePercent()}%"></div>` : ''}
                </div>
            </div>
        `;

        this.elCanvasContent.innerHTML = html;

        this.elCanvasContent.querySelectorAll('.grid-hour-cell').forEach(slot => {
            slot.addEventListener('click', () => {
                this.openCreateModalWithSlot(slot.dataset.date, parseInt(slot.dataset.hour, 10));
            });
        });

        const col = this.elCanvasContent.querySelector(`.grid-day-col[data-date="${dateKey}"]`);
        if (!col) return;

        filtered.forEach(evt => {
            if (!evt.start_time) return;
            const startDate = new Date(evt.start_time);
            if (this.formatDateKey(startDate) !== dateKey) return;

            const hourFrac = startDate.getHours() + (startDate.getMinutes() / 60);
            if (hourFrac < 8 || hourFrac > 21) return;

            const topPx = (hourFrac - 8) * 60;
            let duration = 1;
            if (evt.end_time) {
                duration = Math.max(0.4, (new Date(evt.end_time) - startDate) / (1000 * 60 * 60));
            }
            const heightPx = Math.max(28, duration * 60 - 3);

            const tile = document.createElement('div');
            tile.className = `event-tile color-${this.sanitizeColor(evt.color)}`;
            tile.style.top = `${topPx}px`;
            tile.style.height = `${heightPx}px`;

            tile.innerHTML = `
                <div class="event-tile-title">${this.escapeHtml(evt.title)}</div>
                <div class="event-tile-meta">
                    <span>${this.formatTime(startDate)}</span>
                    ${evt.recurrence && evt.recurrence !== 'none' ? `<span class="event-tile-repeat-badge" title="Repeats: ${this.escapeHtml(this.formatRecurrence(evt.recurrence, evt.start_time))}">${Icons.repeat(11)} <span>${this.escapeHtml(this.formatRecurrence(evt.recurrence, evt.start_time))}</span></span>` : ''}
                    ${this.sanitizeUrl(evt.meeting_url) ? `<span class="event-tile-meet-badge">${Icons.video(12)}<span>Video Call</span></span>` : ''}
                    ${evt.location ? `<span>• ${this.escapeHtml(evt.location)}</span>` : ''}
                </div>
            `;

            tile.addEventListener('click', (e) => {
                e.stopPropagation();
                this.openPopover(evt, tile);
            });
            tile.addEventListener('dblclick', (e) => {
                e.stopPropagation();
                this.openEditModal(evt);
            });

            col.appendChild(tile);
        });
    }

    // --- MONTH VIEW ---
    renderMonthView() {
        const y = this.currentDate.getFullYear();
        const m = this.currentDate.getMonth();
        const firstDay = new Date(y, m, 1);
        const lastDay = new Date(y, m + 1, 0);
        const startDayOfWeek = firstDay.getDay();

        const todayStr = new Date().toDateString();
        const startOfRange = new Date(y, m - 1, 20, 0, 0, 0);
        const endOfRange = new Date(y, m + 1, 15, 23, 59, 59);
        const filtered = this.getEventsForRange(startOfRange, endOfRange);

        let cellsHtml = '';

        // Previous month days
        const prevMonthLastDay = new Date(y, m, 0).getDate();
        for (let i = startDayOfWeek - 1; i >= 0; i--) {
            cellsHtml += `<div class="month-cell other-month"><span class="month-cell-day">${prevMonthLastDay - i}</span></div>`;
        }

        // Current month
        for (let d = 1; d <= lastDay.getDate(); d++) {
            const thisDate = new Date(y, m, d);
            const isToday = thisDate.toDateString() === todayStr;
            const dateKey = this.formatDateKey(thisDate);

            const dayEvents = filtered.filter(e => e.start_time && this.formatDateKey(new Date(e.start_time)) === dateKey);

            cellsHtml += `
                <div class="month-cell ${isToday ? 'today' : ''}" data-date="${dateKey}">
                    <span class="month-cell-day">${d}</span>
                    <div class="month-pills-list">
                        ${dayEvents.slice(0, 3).map(e => `
                            <div class="month-pill color-${this.sanitizeColor(e.color)}" data-id="${this.escapeHtml(e.id)}">
                                ${e.recurrence && e.recurrence !== 'none' ? '🔁 ' : ''}${this.escapeHtml(e.title)}
                            </div>
                        `).join('')}
                        ${dayEvents.length > 3 ? `<span class="caption-label">+${dayEvents.length - 3} more</span>` : ''}
                    </div>
                </div>
            `;
        }

        const html = `
            <div class="month-canvas">
                <div class="month-weekday-row">
                    <span>Sun</span><span>Mon</span><span>Tue</span><span>Wed</span><span>Thu</span><span>Fri</span><span>Sat</span>
                </div>
                <div class="month-grid">
                    ${cellsHtml}
                </div>
            </div>
        `;

        this.elCanvasContent.innerHTML = html;

        this.elCanvasContent.querySelectorAll('.month-pill').forEach(pill => {
            pill.addEventListener('click', (e) => {
                e.stopPropagation();
                const evt = this.events.find(x => x.id === pill.dataset.id);
                if (evt) this.openPopover(evt, pill);
            });
            pill.addEventListener('dblclick', (e) => {
                e.stopPropagation();
                const evt = this.events.find(x => x.id === pill.dataset.id);
                if (evt) this.openEditModal(evt);
            });
        });

        this.elCanvasContent.querySelectorAll('.month-cell').forEach(cell => {
            cell.addEventListener('click', () => {
                if (cell.dataset.date) {
                    this.openCreateModalWithSlot(cell.dataset.date, 10);
                }
            });
        });
    }

    // --- AGENDA VIEW ---
    renderAgendaView() {
        const startOfRange = new Date(this.currentDate);
        startOfRange.setHours(0, 0, 0, 0);
        const endOfRange = new Date(this.currentDate.getTime() + 30 * 24 * 3600 * 1000);
        const filtered = this.getEventsForRange(startOfRange, endOfRange);
        if (filtered.length === 0) {
            this.elCanvasContent.innerHTML = `
                <div class="agenda-canvas">
                    <p style="color: var(--text-muted); text-align: center; padding: 4rem 0;">
                        No calendar meetings scheduled matching filters.<br/>Press <kbd class="kbd-badge">C</kbd> to schedule an event.
                    </p>
                </div>
            `;
            return;
        }

        const sorted = [...filtered].sort((a, b) => (a.start_time || '').localeCompare(b.start_time || ''));
        const groups = {};

        sorted.forEach(e => {
            const dateStr = e.start_time ? new Date(e.start_time).toLocaleDateString(undefined, {
                weekday: 'long', month: 'long', day: 'numeric'
            }) : 'Flexible Date';
            if (!groups[dateStr]) groups[dateStr] = [];
            groups[dateStr].push(e);
        });

        const html = `
            <div class="agenda-canvas">
                ${Object.entries(groups).map(([dateStr, items]) => `
                    <div class="agenda-date-group">
                        <h2 class="agenda-date-header">${dateStr}</h2>
                        <div class="agenda-items-list">
                            ${items.map(e => `
                                <div class="agenda-item-card" data-id="${this.escapeHtml(e.id)}">
                                    <div class="agenda-card-left">
                                        <div class="agenda-time-badge">
                                            ${e.start_time ? this.formatTime(new Date(e.start_time)) : 'All Day'}
                                            ${e.end_time ? ' - ' + this.formatTime(new Date(e.end_time)) : ''}
                                        </div>
                                        <div class="agenda-title-block">
                                            <div class="agenda-title-text">
                                                ${this.escapeHtml(e.title)}
                                                ${e.recurrence && e.recurrence !== 'none' ? `<span class="event-tile-repeat-badge" style="margin-left: 6px;">${Icons.repeat(11)} <span>${this.escapeHtml(this.formatRecurrence(e.recurrence, e.start_time))}</span></span>` : ''}
                                            </div>
                                            <div class="agenda-desc-text">
                                                ${e.location ? `<span class="agenda-location-badge">${Icons.pin(12)}<span>${this.escapeHtml(e.location)}</span></span> • ` : ''}
                                                ${e.description ? this.escapeHtml(e.description) : 'No agenda notes'}
                                            </div>
                                        </div>
                                    </div>
                                    ${this.sanitizeUrl(e.meeting_url) ? `
                                        <a href="${this.escapeHtml(this.sanitizeUrl(e.meeting_url))}" target="_blank" rel="noopener noreferrer" class="btn-join-call" onclick="event.stopPropagation()">
                                            ${Icons.video(13)}
                                            <span>Join Call</span>
                                        </a>
                                    ` : ''}
                                </div>
                            `).join('')}
                        </div>
                    </div>
                `).join('')}
            </div>
        `;

        this.elCanvasContent.innerHTML = html;

        this.elCanvasContent.querySelectorAll('.agenda-item-card').forEach(card => {
            card.addEventListener('click', () => {
                const evt = this.events.find(x => x.id === card.dataset.id);
                if (evt) this.openPopover(evt, card);
            });
            card.addEventListener('dblclick', (e) => {
                e.stopPropagation();
                const evt = this.events.find(x => x.id === card.dataset.id);
                if (evt) this.openEditModal(evt);
            });
        });
    }

    // =========================================================================
    // Mini Month Calendar (Sidebar)
    // =========================================================================
    renderMiniCalendar() {
        const y = this.currentDate.getFullYear();
        const m = this.currentDate.getMonth();
        this.elMiniMonthLabel.textContent = this.currentDate.toLocaleDateString(undefined, { month: 'long', year: 'numeric' });

        const firstDay = new Date(y, m, 1);
        const lastDay = new Date(y, m + 1, 0);
        const startDayOfWeek = firstDay.getDay();

        const today = new Date();
        const isCurrentMonth = today.getFullYear() === y && today.getMonth() === m;

        let html = `
            <div class="mini-day-hdr">S</div>
            <div class="mini-day-hdr">M</div>
            <div class="mini-day-hdr">T</div>
            <div class="mini-day-hdr">W</div>
            <div class="mini-day-hdr">T</div>
            <div class="mini-day-hdr">F</div>
            <div class="mini-day-hdr">S</div>
        `;

        for (let i = 0; i < startDayOfWeek; i++) {
            html += `<div class="mini-day-cell other-month"></div>`;
        }

        const startOfRange = new Date(y, m - 1, 20, 0, 0, 0);
        const endOfRange = new Date(y, m + 1, 15, 23, 59, 59);
        const eventsForMini = this.getEventsForRange(startOfRange, endOfRange);

        for (let d = 1; d <= lastDay.getDate(); d++) {
            const thisDate = new Date(y, m, d);
            const isToday = isCurrentMonth && today.getDate() === d;
            const isSelected = this.currentDate.getDate() === d && this.currentDate.getMonth() === m;
            const dateKey = this.formatDateKey(thisDate);

            const hasEvent = eventsForMini.some(e => e.start_time && this.formatDateKey(new Date(e.start_time)) === dateKey);

            html += `
                <div class="mini-day-cell ${isToday ? 'today' : ''} ${isSelected ? 'selected' : ''} ${hasEvent ? 'has-event' : ''}" data-day="${d}">
                    ${d}
                </div>
            `;
        }

        this.elMiniMonthGrid.innerHTML = html;

        this.elMiniMonthGrid.querySelectorAll('.mini-day-cell[data-day]').forEach(cell => {
            cell.addEventListener('click', () => {
                const day = parseInt(cell.dataset.day, 10);
                this.currentDate.setDate(day);
                if (window.innerWidth < 768) this.closeSidebar();
                this.render();
            });
        });
    }

    // =========================================================================
    // Upcoming Callout & Reminders (Sidebar)
    // =========================================================================
    renderUpcomingCallout() {
        const now = new Date();
        const rangeEnd = new Date(now.getTime() + 14 * 24 * 3600 * 1000);
        const upcomingEvents = this.getEventsForRange(now, rangeEnd);
        const upcoming = upcomingEvents
            .filter(e => e.start_time && new Date(e.start_time) > now)
            .sort((a, b) => a.start_time.localeCompare(b.start_time));

        if (upcoming.length === 0) {
            this.elUpcomingCallout.style.display = 'none';
            return;
        }

        this.elUpcomingCallout.style.display = 'flex';
        const next = upcoming[0];
        const nextDate = new Date(next.start_time);
        const minutesDiff = Math.max(0, Math.round((nextDate - now) / 60000));

        this.elNextMeetingTitle.textContent = next.title;
        this.elNextMeetingTime.textContent = `Today, ${this.formatTime(nextDate)} (in ${minutesDiff}m)`;

        const safeNextUrl = this.sanitizeUrl(next.meeting_url);
        if (safeNextUrl) {
            this.elNextMeetingLink.href = safeNextUrl;
            this.elNextMeetingLink.style.display = 'inline-flex';
        } else {
            this.elNextMeetingLink.removeAttribute('href');
            this.elNextMeetingLink.style.display = 'none';
        }
    }

    renderSmartReminders() {
        const now = new Date();
        const rangeEnd = new Date(now.getTime() + 7 * 24 * 3600 * 1000);
        const upcomingEvents = this.getEventsForRange(now, rangeEnd);
        const upcoming = upcomingEvents
            .filter(e => e.start_time && new Date(e.start_time) > now)
            .sort((a, b) => a.start_time.localeCompare(b.start_time));

        if (upcoming.length === 0) {
            this.elRemindersList.innerHTML = `<span style="font-size: 0.725rem; color: var(--text-muted);">No upcoming reminders active.</span>`;
            return;
        }

        this.elRemindersList.innerHTML = upcoming.slice(0, 3).map(e => {
            const start = new Date(e.start_time);
            return `
                <div class="reminder-chip">
                    <span class="reminder-chip-title">${this.escapeHtml(e.title)}</span>
                    <span class="reminder-chip-time">${Icons.bell(11)}<span>Alert: 15m before (${this.formatTime(start)})</span></span>
                </div>
            `;
        }).join('');
    }

    // =========================================================================
    // Modal & Popover Handling
    // =========================================================================
    openCreateModal() {
        this.editingEventId = null;
        if (this.elModalHeadingTitle) this.elModalHeadingTitle.textContent = 'Schedule Event';
        if (this.elBtnSaveEvent) this.elBtnSaveEvent.textContent = 'Save Event';

        const now = new Date();
        now.setMinutes(0, 0, 0);
        now.setHours(now.getHours() + 1);
        const end = new Date(now.getTime() + 60 * 60 * 1000);

        this.elInputTitle.value = '';
        this.elInputLocation.value = '';
        this.elInputMeeting.value = '';
        this.elInputDesc.value = '';
        this.elInputStart.value = this.toLocalIso(now);
        this.elInputEnd.value = this.toLocalIso(end);
        if (this.elSelectRecurrence) this.elSelectRecurrence.value = 'none';
        this.updateRecurrenceLabels();

        this.closePopover();
        this.resetCustomRecurrence();
        this.elModal.showModal();
        this.elInputTitle.focus();
    }

    openCreateModalWithSlot(dateKey, hour) {
        this.editingEventId = null;
        if (this.elModalHeadingTitle) this.elModalHeadingTitle.textContent = 'Schedule Event';
        if (this.elBtnSaveEvent) this.elBtnSaveEvent.textContent = 'Save Event';

        const [year, month, day] = dateKey.split('-').map(Number);
        const start = new Date(year, month - 1, day, hour, 0, 0);
        const end = new Date(start.getTime() + 60 * 60 * 1000);

        this.elInputTitle.value = '';
        this.elInputLocation.value = '';
        this.elInputMeeting.value = '';
        this.elInputDesc.value = '';
        this.elInputStart.value = this.toLocalIso(start);
        this.elInputEnd.value = this.toLocalIso(end);
        if (this.elSelectRecurrence) this.elSelectRecurrence.value = 'none';
        this.updateRecurrenceLabels();

        this.closePopover();
        this.resetCustomRecurrence();
        this.elModal.showModal();
        this.elInputTitle.focus();
    }

    openEditModal(evt) {
        if (!evt) return;
        this.editingEventId = evt.id;
        if (this.elModalHeadingTitle) this.elModalHeadingTitle.textContent = 'Edit Event';
        if (this.elBtnSaveEvent) this.elBtnSaveEvent.textContent = 'Update Event';

        this.elInputTitle.value = evt.title || '';
        this.elInputLocation.value = evt.location || '';
        this.elInputMeeting.value = evt.meeting_url || '';
        this.elInputDesc.value = evt.description || '';

        const start = evt.start_time ? new Date(evt.start_time) : new Date();
        const end = evt.end_time ? new Date(evt.end_time) : new Date(start.getTime() + 60 * 60 * 1000);
        this.elInputStart.value = this.toLocalIso(start);
        this.elInputEnd.value = this.toLocalIso(end);

        // Color radio chip
        const color = evt.color || 'indigo';
        const colorRadio = document.querySelector(`input[name="color"][value="${color}"]`);
        if (colorRadio) colorRadio.checked = true;

        // Recurrence
        const rec = evt.recurrence || 'none';
        if (rec.startsWith('custom:')) {
            if (this.elSelectRecurrence) this.elSelectRecurrence.value = 'custom';
            const rule = this.parseRecurrenceRule(rec, start);
            if (rule) {
                if (this.elSelectCustomFreq) this.elSelectCustomFreq.value = rule.freq || 'week';
                if (this.elInputCustomInterval) this.elInputCustomInterval.value = rule.interval || 1;
                if (this.elCustomWeekdaysGroup) {
                    this.elCustomWeekdaysGroup.style.display = (rule.freq === 'week') ? 'flex' : 'none';
                }
                if (this.elWeekdayChips) {
                    const daysSet = new Set(rule.days || [start.getDay()]);
                    this.elWeekdayChips.forEach(chip => {
                        const d = parseInt(chip.dataset.day, 10);
                        chip.classList.toggle('active', daysSet.has(d));
                    });
                }
                if (this.elCustomEndsRadios) {
                    this.elCustomEndsRadios.forEach(r => {
                        r.checked = (r.value === (rule.ends || 'never'));
                    });
                }
                if (this.elInputCustomUntil) {
                    this.elInputCustomUntil.disabled = (rule.ends !== 'until');
                    this.elInputCustomUntil.value = rule.until || '';
                }
                if (this.elInputCustomCount) {
                    this.elInputCustomCount.disabled = (rule.ends !== 'count');
                    this.elInputCustomCount.value = rule.count || 10;
                }
                if (this.elCustomRecPanel) this.elCustomRecPanel.classList.remove('hidden');
                this.updateCustomRecurrenceSummary();
            }
        } else {
            if (this.elSelectRecurrence) this.elSelectRecurrence.value = rec;
            this.resetCustomRecurrence();
        }

        this.updateRecurrenceLabels();
        this.closePopover();
        this.elModal.showModal();
        this.elInputTitle.focus();
    }

    updateRecurrenceLabels() {
        if (!this.elInputStart || !this.elInputStart.value) return;
        const dt = new Date(this.elInputStart.value);
        if (isNaN(dt.getTime())) return;

        const dayName = dt.toLocaleDateString(undefined, { weekday: 'long' });
        const dayNum = dt.getDate();

        if (this.elOptWeekly) {
            this.elOptWeekly.textContent = `Weekly on ${dayName}`;
        }
        if (this.elOptBiweekly) {
            this.elOptBiweekly.textContent = `Every 2 weeks on ${dayName}`;
        }
        if (this.elOptMonthly) {
            this.elOptMonthly.textContent = `Monthly on day ${dayNum}`;
        }
    }

    resetCustomRecurrence() {
        if (this.elInputCustomInterval) this.elInputCustomInterval.value = '1';
        if (this.elSelectCustomFreq) this.elSelectCustomFreq.value = 'week';
        if (this.elCustomWeekdaysGroup) this.elCustomWeekdaysGroup.style.display = 'flex';
        if (this.elCustomRecPanel) this.elCustomRecPanel.classList.add('hidden');
        if (this.elInputCustomUntil) {
            this.elInputCustomUntil.value = '';
            this.elInputCustomUntil.disabled = true;
        }
        if (this.elInputCustomCount) {
            this.elInputCustomCount.value = '10';
            this.elInputCustomCount.disabled = true;
        }
        if (this.elCustomEndsRadios) {
            this.elCustomEndsRadios.forEach(r => { r.checked = (r.value === 'never'); });
        }
        this.syncWeekdayChipsWithStartDate();
        this.updateCustomRecurrenceSummary();
    }

    syncWeekdayChipsWithStartDate() {
        if (!this.elInputStart || !this.elInputStart.value || !this.elWeekdayChips) return;
        const dt = new Date(this.elInputStart.value);
        if (isNaN(dt.getTime())) return;
        const day = dt.getDay(); // 0 Sun .. 6 Sat
        const activeCount = Array.from(this.elWeekdayChips).filter(c => c.classList.contains('active')).length;
        if (activeCount === 0 || activeCount === 1) {
            this.elWeekdayChips.forEach(chip => {
                chip.classList.toggle('active', parseInt(chip.dataset.day, 10) === day);
            });
        }
    }

    updateCustomRecurrenceSummary() {
        if (!this.elCustomSummaryText) return;
        const interval = Math.max(1, parseInt(this.elInputCustomInterval?.value, 10) || 1);
        const freq = this.elSelectCustomFreq?.value || 'week';
        const dayNames = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
        const activeDays = [];
        if (this.elWeekdayChips) {
            this.elWeekdayChips.forEach(chip => {
                if (chip.classList.contains('active')) activeDays.push(parseInt(chip.dataset.day, 10));
            });
        }

        let freqPart = '';
        if (interval === 1) {
            if (freq === 'day') freqPart = 'Repeats daily';
            else if (freq === 'week') freqPart = 'Repeats weekly';
            else if (freq === 'month') freqPart = 'Repeats monthly';
            else if (freq === 'year') freqPart = 'Repeats annually';
        } else {
            freqPart = `Repeats every ${interval} ${freq}s`;
        }

        let dayPart = '';
        if (freq === 'week' && activeDays.length > 0) {
            if (activeDays.length === 7) {
                dayPart = ' on all days';
            } else if (activeDays.length === 5 && activeDays.every(d => d >= 1 && d <= 5)) {
                dayPart = ' on weekdays';
            } else {
                dayPart = ' on ' + activeDays.map(d => dayNames[d]).join(', ');
            }
        }

        let endsPart = '';
        const endsRadio = Array.from(this.elCustomEndsRadios || []).find(r => r.checked)?.value || 'never';
        if (endsRadio === 'until' && this.elInputCustomUntil?.value) {
            endsPart = `, ends on ${this.elInputCustomUntil.value}`;
        } else if (endsRadio === 'count') {
            const count = Math.max(1, parseInt(this.elInputCustomCount?.value, 10) || 10);
            endsPart = `, ends after ${count} occurrences`;
        }

        this.elCustomSummaryText.textContent = `${freqPart}${dayPart}${endsPart}`;
    }

    buildCustomRecurrenceString() {
        const interval = Math.max(1, parseInt(this.elInputCustomInterval?.value, 10) || 1);
        const freq = this.elSelectCustomFreq?.value || 'week';
        let days = [];
        if (freq === 'week' && this.elWeekdayChips) {
            this.elWeekdayChips.forEach(chip => {
                if (chip.classList.contains('active')) days.push(parseInt(chip.dataset.day, 10));
            });
            if (days.length === 0) {
                const dt = new Date(this.elInputStart?.value || new Date());
                days = [dt.getDay()];
            }
        }

        const endsRadio = Array.from(this.elCustomEndsRadios || []).find(r => r.checked)?.value || 'never';
        let str = `custom:freq=${freq};interval=${interval}`;
        if (freq === 'week' && days.length > 0) {
            str += `;days=${days.join(',')}`;
        }
        str += `;ends=${endsRadio}`;
        if (endsRadio === 'until' && this.elInputCustomUntil?.value) {
            str += `;until=${this.elInputCustomUntil.value}`;
        } else if (endsRadio === 'count') {
            const count = Math.max(1, parseInt(this.elInputCustomCount?.value, 10) || 10);
            str += `;count=${count}`;
        }
        return str;
    }

    applyDuration(minutes) {
        if (!this.elInputStart.value) return;
        const start = new Date(this.elInputStart.value);
        const end = new Date(start.getTime() + minutes * 60 * 1000);
        this.elInputEnd.value = this.toLocalIso(end);
    }

    openPopover(evt, anchorEl) {
        this.selectedEvent = evt;
        this.elPopoverTitle.textContent = evt.title;

        const start = evt.start_time ? new Date(evt.start_time) : null;
        const end = evt.end_time ? new Date(evt.end_time) : null;

        if (start) {
            const dayName = start.toLocaleDateString(undefined, { weekday: 'long', month: 'short', day: 'numeric' });
            this.elPopoverTime.textContent = `${dayName} • ${this.formatTime(start)} ${end ? ' - ' + this.formatTime(end) : ''}`;
        } else {
            this.elPopoverTime.textContent = 'All Day';
        }

        // Swatch
        this.elPopoverSwatch.className = `popover-swatch swatch-${this.sanitizeColor(evt.color)}`;

        // Join call CTA
        const safeMeetUrl = this.sanitizeUrl(evt.meeting_url);
        if (safeMeetUrl) {
            this.elPopoverJoinBtn.href = safeMeetUrl;
            this.elPopoverJoinBtn.style.display = 'inline-flex';
        } else {
            this.elPopoverJoinBtn.removeAttribute('href');
            this.elPopoverJoinBtn.style.display = 'none';
        }

        // Location
        if (evt.location) {
            this.elPopoverLocation.textContent = evt.location;
            this.elPopoverLocationRow.classList.remove('hidden');
        } else {
            this.elPopoverLocationRow.classList.add('hidden');
        }

        // Recurrence
        if (evt.recurrence && evt.recurrence !== 'none') {
            this.elPopoverRepeatText.textContent = `Repeats: ${this.formatRecurrence(evt.recurrence, evt.start_time)}`;
            this.elPopoverRepeatRow.classList.remove('hidden');
        } else {
            this.elPopoverRepeatRow.classList.add('hidden');
        }

        // Notes
        if (evt.description) {
            this.elPopoverDesc.textContent = evt.description;
            this.elPopoverDescRow.style.display = 'block';
        } else {
            this.elPopoverDescRow.style.display = 'none';
        }

        this.elPopover.classList.remove('hidden');
        if (window.innerWidth < 768 && this.elPopoverBackdrop) {
            this.elPopoverBackdrop.classList.remove('hidden');
        }
    }

    closePopover() {
        this.elPopover.classList.add('hidden');
        if (this.elPopoverBackdrop) {
            this.elPopoverBackdrop.classList.add('hidden');
        }
        this.selectedEvent = null;
    }

    toggleSidebar() {
        if (!this.elSidebarRail) return;
        const isOpen = this.elSidebarRail.classList.toggle('open');
        if (this.elSidebarBackdrop) {
            this.elSidebarBackdrop.classList.toggle('active', isOpen);
        }
    }

    closeSidebar() {
        if (!this.elSidebarRail) return;
        this.elSidebarRail.classList.remove('open');
        if (this.elSidebarBackdrop) {
            this.elSidebarBackdrop.classList.remove('active');
        }
    }

    handleReminderNotification(payload) {
        // 1. Play synthesized bell chime
        this.playBellChime();

        // 2. Animate the header bell icon
        if (this.elHeaderBellIcon) {
            this.elHeaderBellIcon.classList.remove('ringing');
            void this.elHeaderBellIcon.offsetWidth; // trigger reflow
            this.elHeaderBellIcon.classList.add('ringing');
            setTimeout(() => this.elHeaderBellIcon.classList.remove('ringing'), 900);
        }

        // 3. Store notification record
        const notifItem = {
            id: payload.event_id || `notif-${Date.now()}`,
            title: payload.title || 'Event Reminder',
            message: payload.message || `Upcoming reminder for ${payload.title}`,
            start_time: payload.start_time,
            timestamp: new Date(),
            unread: true,
        };
        this.notifications.unshift(notifItem);
        this.unreadNotificationsCount++;
        this.updateNotificationsBadge();
        this.renderNotificationsList();

        // 4. Prominent toast alert with ringing bell icon
        this.showToast('Upcoming Event Reminder', payload.message || payload.title, 'reminder');

        // 5. Native OS/Browser notification if permitted
        if (typeof Notification !== 'undefined' && Notification.permission === 'granted') {
            try {
                new Notification(payload.title || 'Calendar Reminder', {
                    body: payload.message || 'Your event starts soon.',
                    icon: 'data:image/svg+xml,%3Csvg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="%23f59e0b" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"%3E%3Cpath d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/%3E%3Cpath d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/%3E%3C/svg%3E'
                });
            } catch (err) {
                console.warn('Native notification failed', err);
            }
        }
    }

    markNotificationsAsRead() {
        this.unreadNotificationsCount = 0;
        this.updateNotificationsBadge();
        this.notifications.forEach(n => n.unread = false);
        this.renderNotificationsList();
    }

    updateNotificationsBadge() {
        if (!this.elNotificationsBadge) return;
        if (this.unreadNotificationsCount > 0) {
            this.elNotificationsBadge.textContent = this.unreadNotificationsCount > 99 ? '99+' : this.unreadNotificationsCount;
            this.elNotificationsBadge.classList.remove('hidden');
        } else {
            this.elNotificationsBadge.classList.add('hidden');
        }
    }

    renderNotificationsList() {
        if (!this.elNotificationsList) return;
        if (this.notifications.length === 0) {
            this.elNotificationsList.innerHTML = `<div class="notif-empty">No active reminders</div>`;
            return;
        }

        this.elNotificationsList.innerHTML = this.notifications.map(n => {
            const timeAgo = n.timestamp ? n.timestamp.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' }) : '';
            return `
                <div class="notif-item ${n.unread ? 'unread' : ''}">
                    <div class="notif-icon-wrap">${Icons.bell(14)}</div>
                    <div class="notif-content">
                        <div class="notif-title">${this.escapeHtml(n.title)}</div>
                        <div class="notif-subtitle">${this.escapeHtml(n.message)}</div>
                        <div class="notif-time-ago">${timeAgo}</div>
                    </div>
                </div>
            `;
        }).join('');
    }

    playBellChime() {
        try {
            const AudioCtx = window.AudioContext || window.webkitAudioContext;
            if (!AudioCtx) return;
            const ctx = new AudioCtx();
            const now = ctx.currentTime;

            // Two-tone bell harmonic (E5: 659.25Hz -> G#5: 830.6Hz)
            const osc1 = ctx.createOscillator();
            const gain1 = ctx.createGain();
            osc1.type = 'sine';
            osc1.frequency.setValueAtTime(659.25, now);
            gain1.gain.setValueAtTime(0.12, now);
            gain1.gain.exponentialRampToValueAtTime(0.0001, now + 0.55);
            osc1.connect(gain1);
            gain1.connect(ctx.destination);
            osc1.start(now);
            osc1.stop(now + 0.55);

            const osc2 = ctx.createOscillator();
            const gain2 = ctx.createGain();
            osc2.type = 'sine';
            osc2.frequency.setValueAtTime(830.6, now + 0.1);
            gain2.gain.setValueAtTime(0.16, now + 0.1);
            gain2.gain.exponentialRampToValueAtTime(0.0001, now + 0.75);
            osc2.connect(gain2);
            gain2.connect(ctx.destination);
            osc2.start(now + 0.1);
            osc2.stop(now + 0.75);
        } catch (e) {
            // Audio context policy fallback
        }
    }

    // =========================================================================
    // Utilities
    // =========================================================================
    showToast(headline, message, type = 'created') {
        const item = document.createElement('div');
        item.className = `toast-item ${type}`;
        item.innerHTML = `
            <div class="toast-icon-wrap">${type === 'reminder' ? Icons.bell(15) : Icons.calendar(15)}</div>
            <div class="toast-text-wrap">
                <div class="toast-headline">${this.escapeHtml(headline)}</div>
                <div class="toast-msg">${this.escapeHtml(message)}</div>
            </div>
        `;

        this.elToastShelf.appendChild(item);
        setTimeout(() => {
            item.style.opacity = '0';
            item.style.transform = 'translateY(-10px)';
            item.style.transition = 'all 200ms ease';
            setTimeout(() => item.remove(), 200);
        }, 4000);
    }

    logDev(tag, msg) {
        const row = document.createElement('div');
        const time = new Date().toLocaleTimeString();
        row.textContent = `[${time}] [${tag}] ${msg}`;
        this.elInspectorFeed.appendChild(row);
        this.elInspectorFeed.scrollTop = this.elInspectorFeed.scrollHeight;
    }

    getWeekDays(baseDate) {
        const curr = new Date(baseDate);
        const dayOfWeek = curr.getDay(); // 0 is Sun
        const dist = (dayOfWeek + 6) % 7;
        const monday = new Date(curr);
        monday.setDate(curr.getDate() - dist);

        const days = [];
        for (let i = 0; i < 7; i++) {
            const d = new Date(monday);
            d.setDate(monday.getDate() + i);
            days.push(d);
        }
        return days;
    }

    formatDateKey(date) {
        const y = date.getFullYear();
        const m = String(date.getMonth() + 1).padStart(2, '0');
        const d = String(date.getDate()).padStart(2, '0');
        return `${y}-${m}-${d}`;
    }

    formatTime(date) {
        return date.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', hour12: true });
    }

    toLocalIso(date) {
        if (!date || isNaN(date.getTime())) {
            date = new Date();
        }
        const offset = date.getTimezoneOffset();
        const local = new Date(date.getTime() - (offset * 60 * 1000));
        return local.toISOString().slice(0, 16);
    }

    getCurrentTimePercent() {
        const now = new Date();
        const h = now.getHours();
        const m = now.getMinutes();
        if (h < 8) return 0;
        if (h >= 21) return 100;
        return (((h - 8) * 60 + m) / (13 * 60)) * 100;
    }

    escapeHtml(str) {
        if (!str) return '';
        return String(str)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');
    }

    sanitizeColor(color) {
        const allowed = ['indigo', 'emerald', 'sky', 'amber', 'rose', 'purple'];
        return allowed.includes(color) ? color : 'indigo';
    }

    sanitizeUrl(url) {
        if (!url) return '';
        const trimmed = String(url).trim();
        if (/^https?:\/\//i.test(trimmed)) {
            return trimmed;
        }
        return '';
    }
}

// Bootstrap
document.addEventListener('DOMContentLoaded', () => {
    window.app = new CalendarApp();
});
