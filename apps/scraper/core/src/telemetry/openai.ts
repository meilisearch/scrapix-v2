import { telemetry, businessMetrics } from './index'
import { Config } from '../types'
import { trace, SpanStatusCode, context } from '@opentelemetry/api'

interface OpenAIResponse {
  usage?: {
    prompt_tokens: number
    completion_tokens: number
    total_tokens: number
  }
  model?: string
}

export class OpenAITelemetryWrapper {
  /**
   * Wraps an OpenAI API call with telemetry tracking
   * Tracks tokens, costs, and traces
   */
  static async trackOpenAICall<T extends OpenAIResponse>(
    config: Config,
    operation: 'extraction' | 'summary',
    apiCall: () => Promise<T>
  ): Promise<T> {
    const span = telemetry.startSpan(`openai.${operation}`, config, {
      'openai.operation': operation,
      'openai.enabled': true,
    })

    const startTime = Date.now()

    try {
      const result = await context.with(
        trace.setSpan(context.active(), span),
        () => apiCall()
      )

      // Track token usage if available
      if (result.usage) {
        const { prompt_tokens, completion_tokens } = result.usage
        const model = result.model || 'gpt-3.5-turbo'

        // Add span attributes
        span.setAttributes({
          'openai.model': model,
          'openai.tokens.input': prompt_tokens,
          'openai.tokens.output': completion_tokens,
          'openai.tokens.total': prompt_tokens + completion_tokens,
          'openai.duration_ms': Date.now() - startTime,
        })

        // Record business metrics
        businessMetrics.recordOpenAIUsage(
          config,
          model,
          prompt_tokens,
          completion_tokens,
          operation
        )
      }

      span.setStatus({ code: SpanStatusCode.OK })
      return result
    } catch (error) {
      span.recordException(error as Error)
      span.setStatus({
        code: SpanStatusCode.ERROR,
        message: (error as Error).message,
      })
      
      // Track failed attempts as well
      span.setAttributes({
        'openai.error': true,
        'openai.error_type': (error as any).code || 'unknown',
        'openai.duration_ms': Date.now() - startTime,
      })
      
      throw error
    } finally {
      span.end()
    }
  }

  /**
   * Estimates token count for a text (rough approximation)
   * More accurate counting would require tiktoken library
   */
  static estimateTokens(text: string): number {
    // Rough estimation: ~4 characters per token for English text
    return Math.ceil(text.length / 4)
  }

  /**
   * Tracks token usage for streaming responses
   */
  static createStreamTracker(config: Config, operation: 'extraction' | 'summary') {
    let totalInputTokens = 0
    let totalOutputTokens = 0
    const span = telemetry.startSpan(`openai.${operation}.stream`, config)

    return {
      addInput: (text: string) => {
        const tokens = OpenAITelemetryWrapper.estimateTokens(text)
        totalInputTokens += tokens
        span.addEvent('input_chunk', { tokens })
      },
      
      addOutput: (text: string) => {
        const tokens = OpenAITelemetryWrapper.estimateTokens(text)
        totalOutputTokens += tokens
        span.addEvent('output_chunk', { tokens })
      },
      
      finish: (model: string = 'gpt-3.5-turbo') => {
        span.setAttributes({
          'openai.model': model,
          'openai.tokens.input': totalInputTokens,
          'openai.tokens.output': totalOutputTokens,
          'openai.tokens.total': totalInputTokens + totalOutputTokens,
        })
        
        businessMetrics.recordOpenAIUsage(
          config,
          model,
          totalInputTokens,
          totalOutputTokens,
          operation
        )
        
        span.setStatus({ code: SpanStatusCode.OK })
        span.end()
      },
      
      error: (error: Error) => {
        span.recordException(error)
        span.setStatus({
          code: SpanStatusCode.ERROR,
          message: error.message,
        })
        span.end()
      }
    }
  }
}