import type { ComponentType } from 'react';
import type { SvgIconProps } from '@thesvg/react';
import OpenAIIcon from '@thesvg/react/openai';
import ClaudeIcon from '@thesvg/react/claude';
import GeminiIcon from '@thesvg/react/gemini';
import DeepSeekIcon from '@thesvg/react/deepseek';
import MistralIcon from '@thesvg/react/mistral';
import QwenIcon from '@thesvg/react/qwen';
import MetaIcon from '@thesvg/react/meta';
import CohereIcon from '@thesvg/react/cohere';
import PerplexityIcon from '@thesvg/react/perplexity';
import ZhipuIcon from '@thesvg/react/zhipu';
import YiIcon from '@thesvg/react/yi';
import KimiIcon from '@thesvg/react/kimi';
import MinimaxIcon from '@thesvg/react/minimax';
import DoubaoIcon from '@thesvg/react/doubao';
import HunyuanIcon from '@thesvg/react/hunyuan';
import SparkIcon from '@thesvg/react/spark';
import WenxinIcon from '@thesvg/react/wenxin';
import NvidiaIcon from '@thesvg/react/nvidia-nemotron';
import GrokIcon from '@thesvg/react/grok-xai';
import GoogleIcon from '@thesvg/react/google';
import InternLMIcon from '@thesvg/react/internlm';
import StepfunIcon from '@thesvg/react/stepfun';
import GemmaIcon from '@thesvg/react/gemma-google';
import MicrosoftIcon from '@thesvg/react/microsoft';
import KwaiKATIcon from '@thesvg/react/kwaikat-kat-coder';
import XiaomiMimoIcon from '@thesvg/react/xiaomi-mimo';

type ModelIconConfig = {
    keywords: string[];
    Icon: ComponentType<SvgIconProps>;
    className?: string;
    color: string;
};

const MODEL_ICON_PATTERNS: ModelIconConfig[] = [
    { keywords: ['gpt-', 'o1', 'o3', 'o4', 'chatgpt', 'text-embedding', 'dall-e', 'openai'], Icon: OpenAIIcon, className: 'brightness-0 dark:invert', color: '#10A37F' },
    { keywords: ['claude', 'anthropic'], Icon: ClaudeIcon, color: '#D7765A' },
    { keywords: ['gemini'], Icon: GeminiIcon, color: '#4285F4' },
    { keywords: ['gemma'], Icon: GemmaIcon, color: '#4285F4' },
    { keywords: ['palm', 'google'], Icon: GoogleIcon, color: '#4285F4' },
    { keywords: ['xiaomi', 'mimo'], Icon: XiaomiMimoIcon, color: '#FF6900' },
    { keywords: ['deepseek'], Icon: DeepSeekIcon, color: '#4D6BFE' },
    { keywords: ['grok', 'xai'], Icon: GrokIcon, color: '#000000' },
    { keywords: ['qwen', 'qwq', 'alibaba'], Icon: QwenIcon, className: 'brightness-0 dark:invert', color: '#6B4EFF' },
    { keywords: ['glm', 'chatglm', 'zhipu', 'z-ai'], Icon: ZhipuIcon, color: '#3C5BFC' },
    { keywords: ['minimax', 'abab'], Icon: MinimaxIcon, color: '#1A1A2E' },
    { keywords: ['moonshot', 'kimi'], Icon: KimiIcon, color: '#000000' },
    { keywords: ['mistral', 'mixtral', 'codestral', 'pixtral'], Icon: MistralIcon, color: '#F7D046' },
    { keywords: ['llama', 'meta-llama', 'meta'], Icon: MetaIcon, color: '#0668E1' },
    { keywords: ['doubao', 'skylark', 'bytedance'], Icon: DoubaoIcon, color: '#00D6C2' },
    { keywords: ['yi-', '01-ai'], Icon: YiIcon, color: '#1B1464' },
    { keywords: ['hunyuan'], Icon: HunyuanIcon, color: '#0052D9' },
    { keywords: ['spark'], Icon: SparkIcon, color: '#0078FF' },
    { keywords: ['ernie', 'wenxin', 'baidu'], Icon: WenxinIcon, color: '#2932E1' },
    { keywords: ['internlm'], Icon: InternLMIcon, color: '#2F54EB' },
    { keywords: ['stepfun', 'step-'], Icon: StepfunIcon, color: '#5B5CFF' },
    { keywords: ['nvidia', 'nemotron'], Icon: NvidiaIcon, color: '#76B900' },
    { keywords: ['cohere', 'command'], Icon: CohereIcon, color: '#39594D' },
    { keywords: ['perplexity'], Icon: PerplexityIcon, color: '#20B8CD' },
    { keywords: ['phi-'], Icon: MicrosoftIcon, color: '#00BCF2' },
    { keywords: ['kat'], Icon: KwaiKATIcon, color: '#1969FC' },
];

const DEFAULT_CONFIG = { Icon: OpenAIIcon, className: 'brightness-0 dark:invert', color: '#10A37F' };

export function getModelIcon(modelName: string): { Icon: ComponentType<SvgIconProps>; className?: string; color: string } {
    const lowerName = modelName.toLowerCase();
    for (const { keywords, Icon, className, color } of MODEL_ICON_PATTERNS) {
        if (keywords.some(keyword => lowerName.includes(keyword))) {
            return { Icon, className, color };
        }
    }
    return DEFAULT_CONFIG;
}
