#include "viewport_rhi_item.h"
#include "omastudio-app/src/viewport_rhi.cxx.h"

#include <QtCore/QFile>
#include <QtCore/QMetaObject>
#include <QtCore/QPointer>
#include <QtGui/QVulkanInstance>
#include <QtQuick/QQuickWindow>
#include <rhi/qrhi.h>
#include <rhi/qshader.h>

#include <vulkan/vulkan.h>

#include <poll.h>
#include <unistd.h>

#include <algorithm>
#include <cerrno>
#include <cstdint>
#include <cstring>
#include <memory>

namespace {

constexpr std::uint64_t kDrmFormatModInvalid = 0x00ffffffffffffffULL;

QShader load_qsb_shader(const QString &resource_path) {
    QFile f(resource_path);
    if (!f.open(QIODevice::ReadOnly)) {
        return QShader();
    }
    return QShader::fromSerialized(f.readAll());
}

void wait_and_close_sync_fd(int sync_fd) {
    if (sync_fd < 0) {
        return;
    }
    struct pollfd pfd {};
    pfd.fd = sync_fd;
    pfd.events = POLLIN;
    for (;;) {
        const int rc = ::poll(&pfd, 1, 2000);
        if (rc < 0 && errno == EINTR) {
            continue;
        }
        break;
    }
    ::close(sync_fd);
}

struct VulkanFns {
    PFN_vkGetPhysicalDeviceProperties2 getPhysicalDeviceProperties2 = nullptr;
    PFN_vkGetPhysicalDeviceMemoryProperties getPhysicalDeviceMemoryProperties = nullptr;
    PFN_vkCreateImage createImage = nullptr;
    PFN_vkDestroyImage destroyImage = nullptr;
    PFN_vkGetImageMemoryRequirements getImageMemoryRequirements = nullptr;
    PFN_vkAllocateMemory allocateMemory = nullptr;
    PFN_vkFreeMemory freeMemory = nullptr;
    PFN_vkBindImageMemory bindImageMemory = nullptr;
    PFN_vkGetMemoryFdPropertiesKHR getMemoryFdPropertiesKHR = nullptr;
    PFN_vkDeviceWaitIdle deviceWaitIdle = nullptr;

    bool resolve(QVulkanInstance *qvInst, VkDevice dev) {
        if (!qvInst || dev == VK_NULL_HANDLE) {
            return false;
        }
        auto getDevProc = reinterpret_cast<PFN_vkGetDeviceProcAddr>(
            qvInst->getInstanceProcAddr("vkGetDeviceProcAddr")
        );
        if (!getDevProc) {
            return false;
        }
        getPhysicalDeviceProperties2 = reinterpret_cast<PFN_vkGetPhysicalDeviceProperties2>(
            qvInst->getInstanceProcAddr("vkGetPhysicalDeviceProperties2")
        );
        getPhysicalDeviceMemoryProperties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(
            qvInst->getInstanceProcAddr("vkGetPhysicalDeviceMemoryProperties")
        );
        createImage = reinterpret_cast<PFN_vkCreateImage>(getDevProc(dev, "vkCreateImage"));
        destroyImage = reinterpret_cast<PFN_vkDestroyImage>(getDevProc(dev, "vkDestroyImage"));
        getImageMemoryRequirements = reinterpret_cast<PFN_vkGetImageMemoryRequirements>(
            getDevProc(dev, "vkGetImageMemoryRequirements")
        );
        allocateMemory = reinterpret_cast<PFN_vkAllocateMemory>(getDevProc(dev, "vkAllocateMemory"));
        freeMemory = reinterpret_cast<PFN_vkFreeMemory>(getDevProc(dev, "vkFreeMemory"));
        bindImageMemory = reinterpret_cast<PFN_vkBindImageMemory>(getDevProc(dev, "vkBindImageMemory"));
        getMemoryFdPropertiesKHR = reinterpret_cast<PFN_vkGetMemoryFdPropertiesKHR>(
            getDevProc(dev, "vkGetMemoryFdPropertiesKHR")
        );
        deviceWaitIdle = reinterpret_cast<PFN_vkDeviceWaitIdle>(getDevProc(dev, "vkDeviceWaitIdle"));

        return getPhysicalDeviceProperties2 && getPhysicalDeviceMemoryProperties
            && createImage && destroyImage && getImageMemoryRequirements
            && allocateMemory && freeMemory && bindImageMemory
            && getMemoryFdPropertiesKHR && deviceWaitIdle;
    }
};

struct ImportedVkTexture {
    VkDevice dev = VK_NULL_HANDLE;
    VulkanFns fns {};
    VkImage image = VK_NULL_HANDLE;
    VkDeviceMemory memory = VK_NULL_HANDLE;
    std::unique_ptr<QRhiTexture> rhiTexture;
    int width = 0;
    int height = 0;
    int colorSpace = 0;

    void reset() {
        rhiTexture.reset();
        if (dev != VK_NULL_HANDLE) {
            if (fns.deviceWaitIdle) {
                fns.deviceWaitIdle(dev);
            }
            if (image != VK_NULL_HANDLE && fns.destroyImage) {
                fns.destroyImage(dev, image, nullptr);
            }
            if (memory != VK_NULL_HANDLE && fns.freeMemory) {
                fns.freeMemory(dev, memory, nullptr);
            }
        }
        image = VK_NULL_HANDLE;
        memory = VK_NULL_HANDLE;
        width = 0;
        height = 0;
    }

    ~ImportedVkTexture() {
        reset();
    }
};

class OmaRhiViewportRenderer final : public QQuickRhiItemRenderer {
public:
    explicit OmaRhiViewportRenderer(OmaRhiViewport *item)
        : m_item(item) {}

    ~OmaRhiViewportRenderer() override {
        m_imported.reset();
        m_pipeline.reset();
        m_srb.reset();
        m_sampler.reset();
        m_ubo.reset();
        m_fallbackTex.reset();
    }

    void initialize(QRhiCommandBuffer *cb) override {
        Q_UNUSED(cb);
        QRhi *r = rhi();
        if (!r) {
            return;
        }
        if (m_rhi != r) {
            m_imported.reset();
            m_pipeline.reset();
            m_srb.reset();
            m_sampler.reset();
            m_ubo.reset();
            m_fallbackTex.reset();
            m_vkReady = false;
            m_rhi = r;
        }

        if (m_rhi->backend() == QRhi::Vulkan && !m_vkReady) {
            const auto *handles = static_cast<const QRhiVulkanNativeHandles *>(m_rhi->nativeHandles());
            QVulkanInstance *qvInst = (handles && handles->inst)
                ? handles->inst
                : (m_window ? m_window->vulkanInstance() : nullptr);
            if (handles && handles->dev != VK_NULL_HANDLE && handles->physDev != VK_NULL_HANDLE
                && m_vk.resolve(qvInst, handles->dev)) {
                m_vkDev = handles->dev;
                m_vkPhys = handles->physDev;
                VkPhysicalDeviceIDProperties idProps {};
                idProps.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ID_PROPERTIES;
                VkPhysicalDeviceProperties2 props2 {};
                props2.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2;
                props2.pNext = &idProps;
                m_vk.getPhysicalDeviceProperties2(m_vkPhys, &props2);
                std::memcpy(m_deviceUuid, idProps.deviceUUID, sizeof(m_deviceUuid));
                m_vkReady = true;
            }
        }

        if (!m_ubo) {
            m_ubo.reset(m_rhi->newBuffer(QRhiBuffer::Dynamic, QRhiBuffer::UniformBuffer, 16));
            if (!m_ubo->create()) {
                m_ubo.reset();
                return;
            }
        }
        if (!m_sampler) {
            m_sampler.reset(m_rhi->newSampler(
                QRhiSampler::Linear,
                QRhiSampler::Linear,
                QRhiSampler::None,
                QRhiSampler::ClampToEdge,
                QRhiSampler::ClampToEdge
            ));
            if (!m_sampler->create()) {
                m_sampler.reset();
                return;
            }
        }
        if (!m_fallbackTex) {
            m_fallbackTex.reset(m_rhi->newTexture(QRhiTexture::RGBA8, QSize(1, 1), 1));
            if (!m_fallbackTex->create()) {
                m_fallbackTex.reset();
                return;
            }
            m_needClearFallback = true;
        }
        if (!m_srb) {
            QRhiTexture *tex = m_imported && m_imported->rhiTexture
                ? m_imported->rhiTexture.get()
                : m_fallbackTex.get();
            m_srb.reset(m_rhi->newShaderResourceBindings());
            m_srb->setBindings({
                QRhiShaderResourceBinding::uniformBuffer(0, QRhiShaderResourceBinding::VertexStage, m_ubo.get()),
                QRhiShaderResourceBinding::sampledTexture(1, QRhiShaderResourceBinding::FragmentStage, tex, m_sampler.get()),
            });
            if (!m_srb->create()) {
                m_srb.reset();
                return;
            }
        }
        if (!m_pipeline) {
            const QShader vsh = load_qsb_shader(QStringLiteral(":/omastudio/shaders/viewport_rhi.vert.qsb"));
            const QShader fsh = load_qsb_shader(QStringLiteral(":/omastudio/shaders/viewport_rhi.frag.qsb"));
            if (!vsh.isValid() || !fsh.isValid()) {
                return;
            }
            m_pipeline.reset(m_rhi->newGraphicsPipeline());
            m_pipeline->setShaderStages({
                { QRhiShaderStage::Vertex, vsh },
                { QRhiShaderStage::Fragment, fsh },
            });
            m_pipeline->setTopology(QRhiGraphicsPipeline::TriangleStrip);
            m_pipeline->setVertexInputLayout(QRhiVertexInputLayout());
            m_pipeline->setShaderResourceBindings(m_srb.get());
            m_pipeline->setRenderPassDescriptor(renderTarget()->renderPassDescriptor());
            if (!m_pipeline->create()) {
                m_pipeline.reset();
            }
        }
    }

    void synchronize(QQuickRhiItem *item) override {
        auto *vp = static_cast<OmaRhiViewport *>(item);
        m_item = vp;
        m_window = vp ? vp->window() : nullptr;
        const qint64 req = vp ? vp->requestedGeneration() : 0;
        if (req == 0) {
            m_clearRequested = true;
            return;
        }
        if (req != m_lastConsumedGen) {
            m_pendingGen = req;
        }
    }

    void render(QRhiCommandBuffer *cb) override {
        if (!m_rhi || !m_pipeline || !m_srb || !m_ubo) {
            if (m_pendingGen > 0) {
                const PendingDmaBufFrame pending = take_pending_dmabuf_frame();
                if (pending.ok) {
                    if (pending.sync_fd >= 0) {
                        ::close(pending.sync_fd);
                    }
                    if (pending.dmabuf_fd >= 0) {
                        ::close(pending.dmabuf_fd);
                    }
                    m_lastConsumedGen = static_cast<qint64>(pending.generation);
                }
                m_pendingGen = 0;
                if (m_item) {
                    const QPointer<OmaRhiViewport> target = m_item;
                    QMetaObject::invokeMethod(
                        target,
                        [target]() {
                            if (target) {
                                target->reportImportResult(false, 0, 0, 0);
                            }
                        },
                        Qt::QueuedConnection
                    );
                }
            }
            return;
        }

        if (m_clearRequested) {
            m_clearRequested = false;
            m_pendingGen = 0;
            m_lastConsumedGen = 0;
            if (m_imported) {
                m_imported.reset();
                rebindTexture(m_fallbackTex.get());
            }
        }

        if (m_pendingGen > 0 && m_pendingGen != m_lastConsumedGen) {
            const PendingDmaBufFrame pending = take_pending_dmabuf_frame();
            if (pending.ok) {
                m_lastConsumedGen = static_cast<qint64>(pending.generation);
                m_pendingGen = 0;
                const bool imported_ok = importDmaBuf(pending);
                if (m_item) {
                    const QPointer<OmaRhiViewport> target = m_item;
                    const int w = imported_ok ? static_cast<int>(pending.width) : 0;
                    const int h = imported_ok ? static_cast<int>(pending.height) : 0;
                    const int cs = imported_ok ? static_cast<int>(pending.color_space) : 0;
                    QMetaObject::invokeMethod(
                        target,
                        [target, imported_ok, w, h, cs]() {
                            if (target) {
                                target->reportImportResult(imported_ok, w, h, cs);
                            }
                        },
                        Qt::QueuedConnection
                    );
                }
            }
        }

        QRhiResourceUpdateBatch *updates = m_rhi->nextResourceUpdateBatch();
        if (m_needClearFallback && m_fallbackTex) {
            m_needClearFallback = false;
            const std::uint8_t zero_px[4] = { 0, 0, 0, 0 };
            updates->uploadTexture(m_fallbackTex.get(), QImage(zero_px, 1, 1, QImage::Format_RGBA8888));
        }

        const QSize outSize = renderTarget()->pixelSize();
        float sx = 1.0f;
        float sy = 1.0f;
        if (m_imported && m_imported->width > 0 && m_imported->height > 0
            && outSize.width() > 0 && outSize.height() > 0) {
            const float imgAspect = static_cast<float>(m_imported->width) / static_cast<float>(m_imported->height);
            const float dstAspect = static_cast<float>(outSize.width()) / static_cast<float>(outSize.height());
            if (dstAspect > imgAspect) {
                sx = imgAspect / dstAspect;
            } else {
                sy = dstAspect / imgAspect;
            }
        }
        const float yFlip = m_rhi->isYUpInNDC() ? 1.0f : 0.0f;
        const float uboData[4] = { sx, sy, yFlip, 0.0f };
        updates->updateDynamicBuffer(m_ubo.get(), 0, sizeof(uboData), uboData);

        cb->beginPass(renderTarget(), QColor(0, 0, 0, 0), { 1.0f, 0 }, updates);
        if (m_imported && m_imported->rhiTexture) {
            cb->setGraphicsPipeline(m_pipeline.get());
            cb->setViewport(QRhiViewport(0, 0, static_cast<float>(outSize.width()), static_cast<float>(outSize.height())));
            cb->setShaderResources(m_srb.get());
            cb->draw(4);
        }
        cb->endPass();
    }

private:
    void rebindTexture(QRhiTexture *tex) {
        if (!m_srb || !tex) {
            return;
        }
        m_srb->setBindings({
            QRhiShaderResourceBinding::uniformBuffer(0, QRhiShaderResourceBinding::VertexStage, m_ubo.get()),
            QRhiShaderResourceBinding::sampledTexture(1, QRhiShaderResourceBinding::FragmentStage, tex, m_sampler.get()),
        });
        m_srb->create();
    }

    bool importDmaBuf(const PendingDmaBufFrame &frame) {
        const int dmabuf_fd = frame.dmabuf_fd;
        const int sync_fd = frame.sync_fd;

        // Complete explicit synchronization before closing sync_fd and importing the image.
        wait_and_close_sync_fd(sync_fd);

        auto close_dmabuf = [dmabuf_fd]() {
            if (dmabuf_fd >= 0) {
                ::close(dmabuf_fd);
            }
        };

        if (!m_vkReady || dmabuf_fd < 0
            || frame.width == 0 || frame.height == 0
            || frame.width > 16384 || frame.height > 16384
            || frame.device_uuid.size() != 16) {
            close_dmabuf();
            return false;
        }

        // Reject cross-GPU DMA-BUF import when producer and consumer use different physical devices.
        if (std::memcmp(m_deviceUuid, frame.device_uuid.data(), 16) != 0) {
            close_dmabuf();
            return false;
        }

        VkExternalMemoryImageCreateInfo extInfo {};
        extInfo.sType = VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO;
        extInfo.handleTypes = VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT;

        VkSubresourceLayout planeLayout {};
        planeLayout.offset = frame.offset;
        planeLayout.rowPitch = frame.stride;
        planeLayout.size = 0;

        VkImageDrmFormatModifierExplicitCreateInfoEXT modInfo {};
        modInfo.sType = VK_STRUCTURE_TYPE_IMAGE_DRM_FORMAT_MODIFIER_EXPLICIT_CREATE_INFO_EXT;
        modInfo.drmFormatModifier = frame.drm_modifier;
        modInfo.drmFormatModifierPlaneCount = 1;
        modInfo.pPlaneLayouts = &planeLayout;

        const bool useModifier = (frame.drm_modifier != kDrmFormatModInvalid);
        if (useModifier) {
            extInfo.pNext = &modInfo;
        }

        VkImageCreateInfo imgInfo {};
        imgInfo.sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO;
        imgInfo.pNext = &extInfo;
        imgInfo.imageType = VK_IMAGE_TYPE_2D;
        imgInfo.format = static_cast<VkFormat>(frame.vk_format);
        imgInfo.extent = { frame.width, frame.height, 1 };
        imgInfo.mipLevels = 1;
        imgInfo.arrayLayers = 1;
        imgInfo.samples = VK_SAMPLE_COUNT_1_BIT;
        imgInfo.tiling = useModifier ? VK_IMAGE_TILING_DRM_FORMAT_MODIFIER_EXT : VK_IMAGE_TILING_LINEAR;
        imgInfo.usage = VK_IMAGE_USAGE_SAMPLED_BIT;
        imgInfo.sharingMode = VK_SHARING_MODE_EXCLUSIVE;
        imgInfo.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;

        VkImage vkImage = VK_NULL_HANDLE;
        if (m_vk.createImage(m_vkDev, &imgInfo, nullptr, &vkImage) != VK_SUCCESS) {
            close_dmabuf();
            return false;
        }

        VkMemoryRequirements memReqs {};
        m_vk.getImageMemoryRequirements(m_vkDev, vkImage, &memReqs);

        VkMemoryFdPropertiesKHR fdProps {};
        fdProps.sType = VK_STRUCTURE_TYPE_MEMORY_FD_PROPERTIES_KHR;
        if (m_vk.getMemoryFdPropertiesKHR(
                m_vkDev,
                VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
                dmabuf_fd,
                &fdProps
            ) != VK_SUCCESS) {
            m_vk.destroyImage(m_vkDev, vkImage, nullptr);
            close_dmabuf();
            return false;
        }

        const std::uint32_t validTypes = memReqs.memoryTypeBits & fdProps.memoryTypeBits;
        VkPhysicalDeviceMemoryProperties devMemProps {};
        m_vk.getPhysicalDeviceMemoryProperties(m_vkPhys, &devMemProps);
        std::uint32_t chosenType = UINT32_MAX;
        for (std::uint32_t i = 0; i < devMemProps.memoryTypeCount; ++i) {
            if ((validTypes & (1u << i)) != 0) {
                chosenType = i;
                if ((devMemProps.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT) != 0) {
                    break;
                }
            }
        }
        if (chosenType == UINT32_MAX) {
            m_vk.destroyImage(m_vkDev, vkImage, nullptr);
            close_dmabuf();
            return false;
        }

        VkImportMemoryFdInfoKHR importInfo {};
        importInfo.sType = VK_STRUCTURE_TYPE_IMPORT_MEMORY_FD_INFO_KHR;
        importInfo.handleType = VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT;
        importInfo.fd = dmabuf_fd;

        VkMemoryDedicatedAllocateInfo dedicatedInfo {};
        dedicatedInfo.sType = VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO;
        dedicatedInfo.image = vkImage;
        importInfo.pNext = &dedicatedInfo;

        VkMemoryAllocateInfo allocInfo {};
        allocInfo.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO;
        allocInfo.pNext = &importInfo;
        allocInfo.allocationSize = std::max<VkDeviceSize>(memReqs.size, static_cast<VkDeviceSize>(frame.alloc_size));
        allocInfo.memoryTypeIndex = chosenType;

        VkDeviceMemory vkMem = VK_NULL_HANDLE;
        if (m_vk.allocateMemory(m_vkDev, &allocInfo, nullptr, &vkMem) != VK_SUCCESS) {
            // Vulkan only takes ownership of dmabuf_fd on VK_SUCCESS.
            m_vk.destroyImage(m_vkDev, vkImage, nullptr);
            close_dmabuf();
            return false;
        }
        // dmabuf_fd is now owned by vkMem.

        if (m_vk.bindImageMemory(m_vkDev, vkImage, vkMem, 0) != VK_SUCCESS) {
            m_vk.destroyImage(m_vkDev, vkImage, nullptr);
            m_vk.freeMemory(m_vkDev, vkMem, nullptr);
            return false;
        }

        std::unique_ptr<QRhiTexture> rhiTex(
            m_rhi->newTexture(QRhiTexture::RGBA16F, QSize(static_cast<int>(frame.width), static_cast<int>(frame.height)), 1)
        );
        QRhiTexture::NativeTexture nativeTex {};
        nativeTex.object = static_cast<quint64>(reinterpret_cast<std::uintptr_t>(vkImage));
        // Producer compute pass (`wgpu::StorageTexture`) leaves the image in VK_IMAGE_LAYOUT_GENERAL.
        nativeTex.layout = VK_IMAGE_LAYOUT_GENERAL;
        if (!rhiTex || !rhiTex->createFrom(nativeTex)) {
            rhiTex.reset();
            m_vk.destroyImage(m_vkDev, vkImage, nullptr);
            m_vk.freeMemory(m_vkDev, vkMem, nullptr);
            return false;
        }

        auto next = std::make_unique<ImportedVkTexture>();
        next->dev = m_vkDev;
        next->fns = m_vk;
        next->image = vkImage;
        next->memory = vkMem;
        next->rhiTexture = std::move(rhiTex);
        next->width = static_cast<int>(frame.width);
        next->height = static_cast<int>(frame.height);
        next->colorSpace = static_cast<int>(frame.color_space);

        rebindTexture(next->rhiTexture.get());
        m_imported = std::move(next);
        return true;
    }

    QPointer<OmaRhiViewport> m_item;
    QPointer<QQuickWindow> m_window;
    QRhi *m_rhi = nullptr;
    bool m_vkReady = false;
    VkDevice m_vkDev = VK_NULL_HANDLE;
    VkPhysicalDevice m_vkPhys = VK_NULL_HANDLE;
    VulkanFns m_vk {};
    std::uint8_t m_deviceUuid[16] = {};

    std::unique_ptr<QRhiBuffer> m_ubo;
    std::unique_ptr<QRhiSampler> m_sampler;
    std::unique_ptr<QRhiTexture> m_fallbackTex;
    std::unique_ptr<QRhiShaderResourceBindings> m_srb;
    std::unique_ptr<QRhiGraphicsPipeline> m_pipeline;
    std::unique_ptr<ImportedVkTexture> m_imported;

    bool m_needClearFallback = false;
    bool m_clearRequested = false;
    qint64 m_pendingGen = 0;
    qint64 m_lastConsumedGen = 0;
};

} // namespace

OmaRhiViewport::OmaRhiViewport(QQuickItem *parent)
    : QQuickRhiItem(parent) {
    setColorBufferFormat(QQuickRhiItem::TextureFormat::RGBA16F);
    setAlphaBlending(true);
}

QQuickRhiItemRenderer *OmaRhiViewport::createRenderer() {
    return new OmaRhiViewportRenderer(this);
}

void OmaRhiViewport::presentFrame(qint64 generation, int width, int height, int colorSpace) {
    m_requestedGen = generation;
    if (m_frameWidth != width || m_frameHeight != height || m_colorSpace != colorSpace) {
        m_frameWidth = width;
        m_frameHeight = height;
        m_colorSpace = colorSpace;
        emit frameMetricsChanged();
    }
    update();
}

void OmaRhiViewport::clearFrame() {
    m_requestedGen = 0;
    if (m_dmaBufActive) {
        m_dmaBufActive = false;
        emit dmaBufActiveChanged();
    }
    if (m_frameWidth != 0 || m_frameHeight != 0) {
        m_frameWidth = 0;
        m_frameHeight = 0;
        emit frameMetricsChanged();
    }
    update();
}

void OmaRhiViewport::reportImportResult(bool ok, int width, int height, int colorSpace) {
    if (ok) {
        if (m_frameWidth != width || m_frameHeight != height || m_colorSpace != colorSpace) {
            m_frameWidth = width;
            m_frameHeight = height;
            m_colorSpace = colorSpace;
            emit frameMetricsChanged();
        }
        if (!m_dmaBufActive) {
            m_dmaBufActive = true;
            emit dmaBufActiveChanged();
        }
    } else {
        if (m_dmaBufActive) {
            m_dmaBufActive = false;
            emit dmaBufActiveChanged();
        }
        emit dmaBufFailed();
    }
}
