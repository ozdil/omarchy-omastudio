/**
 * OmaSend Web - Modern Fluid Glassmorphism UI Logic
 * Omarchy Zero-Emoji Compliant
 * Font: JetBrainsMono Nerd Font
 */

document.addEventListener('DOMContentLoaded', () => {
  // State
  const state = {
    myOmaId: 'OMA-8492-1048-5729',
    selectedDeviceId: 'dev-1',
    activeTab: 'files',
    devices: [
      { id: 'dev-1', name: 'omarchy-thinkpad', type: 'LAN Direct', protocol: 'mDNS + TLS', signal: '100%', ip: '192.168.1.142', status: 'Cevrimici' },
      { id: 'dev-2', name: 'kastamonu-node-01', type: 'OmaID P2P', protocol: 'QUIC + ChaCha20', signal: '92%', ip: '10.88.0.4', status: 'Bagli' },
      { id: 'dev-3', name: 'pixel-9-pro', type: 'LAN Direct', protocol: 'WebRTC / P2P', signal: '88%', ip: '192.168.1.189', status: 'Cevrimici' }
    ],
    transfers: [
      {
        id: 'tx-1',
        filename: 'omastudio-raw-pack-2026.tar.zst',
        size: '1.42 GB',
        target: 'omarchy-thinkpad',
        progress: 68,
        speed: '142.8 MB/s',
        eta: '3s',
        status: 'Aktariliyor'
      }
    ],
    clipboardItems: [
      {
        id: 'clip-1',
        type: 'text',
        title: 'Git Commit Hash',
        content: 'f8a92e104b78c93de8219fa82110c4d8',
        time: '2 dk once',
        secure: true
      },
      {
        id: 'clip-2',
        type: 'url',
        title: 'OmaSend Rendezvous Endpoint',
        content: 'omasend://rendezvous.omarchy.local:8080/sync/4829',
        time: '8 dk once',
        secure: true
      },
      {
        id: 'clip-3',
        type: 'token',
        title: 'Zero-Trust Session Token',
        content: 'eyJhbGciOiJDaGFDaGEyMC1Qb2x5MTMwNSIsInR5cCI6IkpXVCJ9.oma.sig',
        time: '14 dk once',
        secure: true
      }
    ]
  };

  // DOM Elements
  const deviceListEl = document.getElementById('deviceList');
  const fileDropZone = document.getElementById('fileDropZone');
  const fileInput = document.getElementById('fileInput');
  const transferContainer = document.getElementById('transferContainer');
  const clipboardGrid = document.getElementById('clipboardGrid');
  const tabButtons = document.querySelectorAll('.tab-btn');
  const filesView = document.getElementById('filesView');
  const clipboardView = document.getElementById('clipboardView');

  // Modals
  const omaIdModal = document.getElementById('omaIdModal');
  const qrScannerModal = document.getElementById('qrScannerModal');
  const aboutModal = document.getElementById('aboutModal');

  // Initialize UI
  renderDevices();
  renderTransfers();
  renderClipboard();
  initOmaIdQr();

  // Tab Navigation
  tabButtons.forEach(btn => {
    btn.addEventListener('click', () => {
      tabButtons.forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      const targetTab = btn.getAttribute('data-tab');
      state.activeTab = targetTab;

      if (targetTab === 'files') {
        filesView.style.display = 'block';
        clipboardView.style.display = 'none';
      } else {
        filesView.style.display = 'none';
        clipboardView.style.display = 'block';
      }
    });
  });

  // Render Devices
  function renderDevices() {
    if (!deviceListEl) return;
    deviceListEl.innerHTML = state.devices.map(dev => `
      <div class="device-card-item ${dev.id === state.selectedDeviceId ? 'selected' : ''}" data-device-id="${dev.id}">
        <div class="device-card-main">
          <div class="device-avatar-icon">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <rect x="2" y="3" width="20" height="14" rx="2" ry="2"></rect>
              <line x1="8" y1="21" x2="16" y2="21"></line>
              <line x1="12" y1="17" x2="12" y2="21"></line>
            </svg>
          </div>
          <div class="device-text-meta">
            <span class="device-name">${escapeHtml(dev.name)}</span>
            <span class="device-status-sub">${escapeHtml(dev.ip)} • ${escapeHtml(dev.status)}</span>
          </div>
        </div>
        <div class="device-meta-badges">
          <span class="protocol-tag ${dev.type.includes('LAN') ? 'lan' : ''}">${escapeHtml(dev.type)}</span>
          <span class="signal-bars">${escapeHtml(dev.signal)}</span>
        </div>
      </div>
    `).join('');

    // Attach click events
    document.querySelectorAll('.device-card-item').forEach(card => {
      card.addEventListener('click', () => {
        const id = card.getAttribute('data-device-id');
        state.selectedDeviceId = id;
        renderDevices();
      });
    });
  }

  // Render Active Transfers
  function renderTransfers() {
    if (!transferContainer) return;
    transferContainer.innerHTML = state.transfers.map(tx => `
      <div class="transfer-item-card" id="${tx.id}">
        <div class="transfer-item-header">
          <div class="transfer-file-info">
            <div class="file-type-icon">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"></path>
                <polyline points="13 2 13 9 20 9"></polyline>
              </svg>
            </div>
            <div class="file-name-meta">
              <span class="transfer-filename">${escapeHtml(tx.filename)}</span>
              <span class="transfer-target-info">Hedef: ${escapeHtml(tx.target)} • ${tx.size}</span>
            </div>
          </div>
          <div class="transfer-telemetry">
            <span class="telemetry-stat">Hiz: <strong>${tx.speed}</strong></span>
            <span class="telemetry-stat">ETA: <strong>${tx.eta}</strong></span>
            <span class="telemetry-stat"><strong>%${tx.progress}</strong></span>
          </div>
        </div>
        <div class="transfer-progress-track">
          <div class="transfer-progress-fill" style="width: ${tx.progress}%"></div>
        </div>
      </div>
    `).join('');
  }

  // Render Clipboard Vault
  function renderClipboard() {
    if (!clipboardGrid) return;
    clipboardGrid.innerHTML = state.clipboardItems.map(item => `
      <div class="clip-card" data-clip-id="${item.id}">
        <div class="clip-header">
          <span style="font-weight: 600; color: var(--accent-cyan-light);">${escapeHtml(item.title)}</span>
          <span>${escapeHtml(item.time)}</span>
        </div>
        <div class="clip-content-box">${escapeHtml(item.content)}</div>
        <div class="clip-actions">
          <span style="font-size: 0.68rem; color: var(--accent-green); display: flex; align-items: center; gap: 4px;">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect>
              <path d="M7 11V7a5 5 0 0 1 10 0v4"></path>
            </svg>
            Uctan Uca Sifreli
          </span>
          <button class="copy-btn" onclick="copyClipContent('${escapeJsString(item.content)}', this)">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
              <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
            </svg>
            Kopyala
          </button>
        </div>
      </div>
    `).join('');
  }

  // Drag and Drop Liquid Portal
  if (fileDropZone) {
    ['dragenter', 'dragover'].forEach(eventName => {
      fileDropZone.addEventListener(eventName, (e) => {
        e.preventDefault();
        e.stopPropagation();
        fileDropZone.classList.add('drag-over');
      }, false);
    });

    ['dragleave', 'drop'].forEach(eventName => {
      fileDropZone.addEventListener(eventName, (e) => {
        e.preventDefault();
        e.stopPropagation();
        fileDropZone.classList.remove('drag-over');
      }, false);
    });

    fileDropZone.addEventListener('drop', (e) => {
      const dt = e.dataTransfer;
      const files = dt.files;
      handleFilesSelected(files);
    });

    fileDropZone.addEventListener('click', () => {
      if (fileInput) fileInput.click();
    });
  }

  if (fileInput) {
    fileInput.addEventListener('change', (e) => {
      handleFilesSelected(e.target.files);
    });
  }

  function handleFilesSelected(files) {
    if (!files || files.length === 0) return;
    const targetDev = state.devices.find(d => d.id === state.selectedDeviceId) || state.devices[0];

    for (let i = 0; i < files.length; i++) {
      const file = files[i];
      const newTx = {
        id: 'tx-' + Date.now() + '-' + i,
        filename: file.name,
        size: formatBytes(file.size),
        target: targetDev.name,
        progress: 0,
        speed: 'Hesaplaniyor...',
        eta: '...',
        status: 'Baslatiliyor'
      };
      state.transfers.unshift(newTx);
      simulateTransfer(newTx.id);
    }
    renderTransfers();
  }

  function simulateTransfer(txId) {
    let progress = 0;
    const interval = setInterval(() => {
      progress += Math.floor(Math.random() * 12) + 6;
      if (progress >= 100) {
        progress = 100;
        clearInterval(interval);
      }
      const tx = state.transfers.find(t => t.id === txId);
      if (tx) {
        tx.progress = progress;
        tx.speed = (110 + Math.random() * 45).toFixed(1) + ' MB/s';
        const remaining = Math.max(0, Math.ceil((100 - progress) / 25));
        tx.eta = remaining + 's';
        if (progress === 100) {
          tx.status = 'Tamamlandi';
          tx.speed = 'Tamamlandi';
          tx.eta = '0s';
        }
        renderTransfers();
      }
    }, 400);
  }

  // OmaID SVG QR Generation (Clean SVG matrix)
  function initOmaIdQr() {
    const qrSvgBox = document.getElementById('omaQrBox');
    if (!qrSvgBox) return;

    // Generate precision stylized SVG QR pattern
    qrSvgBox.innerHTML = `
      <svg viewBox="0 0 100 100" fill="currentColor">
        <!-- Corner Finder Patterns -->
        <rect x="5" y="5" width="26" height="26" fill="#0f172a" rx="4" />
        <rect x="9" y="9" width="18" height="18" fill="#ffffff" rx="2" />
        <rect x="13" y="13" width="10" height="10" fill="#0284c7" rx="1" />

        <rect x="69" y="5" width="26" height="26" fill="#0f172a" rx="4" />
        <rect x="73" y="9" width="18" height="18" fill="#ffffff" rx="2" />
        <rect x="77" y="13" width="10" height="10" fill="#0284c7" rx="1" />

        <rect x="5" y="69" width="26" height="26" fill="#0f172a" rx="4" />
        <rect x="9" y="73" width="18" height="18" fill="#ffffff" rx="2" />
        <rect x="13" y="77" width="10" height="10" fill="#0284c7" rx="1" />

        <!-- Data Matrix Bits -->
        <rect x="36" y="8" width="5" height="5" fill="#0f172a" />
        <rect x="46" y="8" width="5" height="5" fill="#0f172a" />
        <rect x="56" y="8" width="5" height="5" fill="#0f172a" />
        <rect x="36" y="18" width="5" height="5" fill="#0284c7" />
        <rect x="46" y="18" width="15" height="5" fill="#0f172a" />
        
        <rect x="8" y="36" width="5" height="5" fill="#0f172a" />
        <rect x="18" y="36" width="5" height="5" fill="#0284c7" />
        <rect x="28" y="36" width="5" height="5" fill="#0f172a" />
        <rect x="38" y="36" width="25" height="5" fill="#0f172a" />
        <rect x="68" y="36" width="5" height="5" fill="#0284c7" />
        <rect x="78" y="36" width="14" height="5" fill="#0f172a" />

        <rect x="8" y="46" width="15" height="5" fill="#0f172a" />
        <rect x="28" y="46" width="5" height="5" fill="#0f172a" />
        <rect x="38" y="46" width="10" height="5" fill="#0284c7" />
        <rect x="53" y="46" width="10" height="5" fill="#0f172a" />
        <rect x="68" y="46" width="10" height="5" fill="#0f172a" />
        <rect x="83" y="46" width="9" height="5" fill="#0284c7" />

        <rect x="8" y="56" width="5" height="5" fill="#0284c7" />
        <rect x="18" y="56" width="15" height="5" fill="#0f172a" />
        <rect x="38" y="56" width="5" height="5" fill="#0f172a" />
        <rect x="48" y="56" width="15" height="5" fill="#0284c7" />
        <rect x="68" y="56" width="5" height="5" fill="#0f172a" />
        <rect x="78" y="56" width="14" height="5" fill="#0f172a" />

        <rect x="36" y="68" width="10" height="5" fill="#0f172a" />
        <rect x="51" y="68" width="5" height="5" fill="#0284c7" />
        <rect x="61" y="68" width="15" height="5" fill="#0f172a" />
        <rect x="81" y="68" width="11" height="5" fill="#0f172a" />

        <rect x="36" y="78" width="20" height="5" fill="#0f172a" />
        <rect x="61" y="78" width="5" height="5" fill="#0284c7" />
        <rect x="71" y="78" width="21" height="5" fill="#0f172a" />

        <rect x="36" y="88" width="5" height="5" fill="#0f172a" />
        <rect x="46" y="88" width="10" height="5" fill="#0284c7" />
        <rect x="61" y="88" width="15" height="5" fill="#0f172a" />
        <rect x="81" y="88" width="11" height="5" fill="#0284c7" />
      </svg>
    `;
  }

  // Modal open/close bindings
  window.openOmaIdModal = () => omaIdModal.classList.add('active');
  window.closeOmaIdModal = () => omaIdModal.classList.remove('active');

  window.openQrScannerModal = () => qrScannerModal.classList.add('active');
  window.closeQrScannerModal = () => qrScannerModal.classList.remove('active');

  window.openAboutModal = () => aboutModal.classList.add('active');
  window.closeAboutModal = () => aboutModal.classList.remove('active');

  // Copy helper
  window.copyClipContent = (text, btnElement) => {
    navigator.clipboard.writeText(text).then(() => {
      const originalHtml = btnElement.innerHTML;
      btnElement.classList.add('copied');
      btnElement.innerHTML = `
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="20 6 9 17 4 12"></polyline>
        </svg>
        Kopyalandi
      `;
      setTimeout(() => {
        btnElement.classList.remove('copied');
        btnElement.innerHTML = originalHtml;
      }, 2000);
    });
  };

  // Utilities
  function formatBytes(bytes) {
    if (bytes === 0) return '0 Bytes';
    const k = 1024;
    const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
  }

  function escapeHtml(str) {
    return String(str)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#039;');
  }

  function escapeJsString(str) {
    return String(str)
      .replace(/\\/g, '\\\\')
      .replace(/'/g, "\\'")
      .replace(/"/g, '\\"')
      .replace(/\n/g, '\\n')
      .replace(/\r/g, '\\r');
  }
});
