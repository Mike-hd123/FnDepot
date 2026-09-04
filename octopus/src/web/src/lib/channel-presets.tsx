import type { ComponentType } from 'react';
import type { SvgIconProps } from '@thesvg/react';
import NewAPIIcon from '@thesvg/react/new-api';
import OpenAIIcon from '@thesvg/react/openai-chatgpt';
import AnthropicIcon from '@thesvg/react/anthropic';
import VolcengineIcon from '@thesvg/react/volcengine';
import DeepSeekIcon from '@thesvg/react/deepseek';
import OpenRouterIcon from '@thesvg/react/openrouter';
import GroqIcon from '@thesvg/react/groq';
import QwenIcon from '@thesvg/react/qwen';
import MoonshotIcon from '@thesvg/react/moonshot-ai';
import ZhipuIcon from '@thesvg/react/zhipu';
import XAIIcon from '@thesvg/react/xai-grok';
import SiliconFlowIcon from '@thesvg/react/siliconcloud-siliconflow';
import AzureIcon from '@thesvg/react/azure-azure-openai';
import type { Dialect } from '@/api/channel';

// ChannelPreset 是服务商的地址, 路径与方言预填模板。
// 地址与路径只在前端存在, 不落库: 这些服务商对后端没有区别, 只是地址和路径不同。
// 方言随模板给出: 它表达的是该服务商在标准协议之上的差异, 属于服务商固有属性, 由用户另选没有意义。
export type ChannelPreset = {
    id: string;
    label: string;
    Icon: ComponentType<SvgIconProps>;
    iconClassName?: string; // 单色图标在深色主题下需反色。
    dialect: Dialect;
    base_url: string;
    openai_chat_completion_path: string;
    openai_response_path: string;
    anthropic_message_path: string;
};

// 默认协议路径, 与后端 DDL 默认值一致。
const CHAT = '/v1/chat/completions';
const RESP = '/v1/responses';
const ANTH = '/v1/messages';

export const CHANNEL_PRESETS: ChannelPreset[] = [
    {
        id: 'newapi', label: 'New API', Icon: NewAPIIcon,
        dialect: 'generic',
        base_url: '',
        openai_chat_completion_path: CHAT, openai_response_path: RESP, anthropic_message_path: ANTH,
    },
    {
        id: 'openai', label: 'OpenAI', Icon: OpenAIIcon, iconClassName: 'brightness-0 dark:invert',
        dialect: 'generic',
        base_url: 'https://api.openai.com',
        openai_chat_completion_path: CHAT, openai_response_path: RESP, anthropic_message_path: ANTH,
    },
    {
        id: 'anthropic', label: 'Anthropic', Icon: AnthropicIcon, iconClassName: 'brightness-0 dark:invert',
        dialect: 'generic',
        base_url: 'https://api.anthropic.com',
        openai_chat_completion_path: CHAT, openai_response_path: RESP, anthropic_message_path: ANTH,
    },
    {
        id: 'volcengine', label: '火山方舟', Icon: VolcengineIcon,
        dialect: 'generic',
        base_url: 'https://ark.cn-beijing.volces.com/api/v3',
        openai_chat_completion_path: '/chat/completions', openai_response_path: '/responses', anthropic_message_path: '/messages',
    },
    {
        id: 'deepseek', label: 'DeepSeek', Icon: DeepSeekIcon,
        dialect: 'generic',
        base_url: 'https://api.deepseek.com',
        openai_chat_completion_path: CHAT, openai_response_path: RESP, anthropic_message_path: ANTH,
    },
    {
        id: 'openrouter', label: 'OpenRouter', Icon: OpenRouterIcon, iconClassName: 'brightness-0 dark:invert',
        dialect: 'generic',
        base_url: 'https://openrouter.ai/api',
        openai_chat_completion_path: CHAT, openai_response_path: RESP, anthropic_message_path: ANTH,
    },
    {
        id: 'groq', label: 'Groq', Icon: GroqIcon,
        dialect: 'generic',
        base_url: 'https://api.groq.com/openai',
        openai_chat_completion_path: CHAT, openai_response_path: RESP, anthropic_message_path: ANTH,
    },
    {
        id: 'dashscope', label: '通义千问', Icon: QwenIcon, iconClassName: 'brightness-0 dark:invert',
        dialect: 'generic',
        base_url: 'https://dashscope.aliyuncs.com/compatible-mode',
        openai_chat_completion_path: CHAT, openai_response_path: RESP, anthropic_message_path: ANTH,
    },
    {
        id: 'moonshot', label: 'Moonshot', Icon: MoonshotIcon, iconClassName: 'brightness-0 dark:invert',
        dialect: 'generic',
        base_url: 'https://api.moonshot.cn',
        openai_chat_completion_path: CHAT, openai_response_path: RESP, anthropic_message_path: ANTH,
    },
    {
        id: 'zhipu', label: '智谱 GLM', Icon: ZhipuIcon,
        dialect: 'generic',
        base_url: 'https://open.bigmodel.cn/api/paas/v4',
        openai_chat_completion_path: '/chat/completions', openai_response_path: '/responses', anthropic_message_path: '/messages',
    },
    {
        id: 'xai', label: 'xAI', Icon: XAIIcon, iconClassName: 'brightness-0 dark:invert',
        dialect: 'generic',
        base_url: 'https://api.x.ai',
        openai_chat_completion_path: CHAT, openai_response_path: RESP, anthropic_message_path: ANTH,
    },
    {
        id: 'siliconflow', label: 'SiliconFlow', Icon: SiliconFlowIcon,
        dialect: 'generic',
        base_url: 'https://api.siliconflow.cn',
        openai_chat_completion_path: CHAT, openai_response_path: RESP, anthropic_message_path: ANTH,
    },
    {
        id: 'azure', label: 'Azure OpenAI', Icon: AzureIcon,
        dialect: 'generic',
        base_url: '',
        openai_chat_completion_path: CHAT, openai_response_path: RESP, anthropic_message_path: ANTH,
    },
];
