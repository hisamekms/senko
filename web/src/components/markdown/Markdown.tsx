import ReactMarkdown from 'react-markdown'
import rehypeSanitize from 'rehype-sanitize'
import remarkGfm from 'remark-gfm'

import { css } from '../../../styled-system/css'

const proseStyle = css({
  display: 'flex',
  flexDirection: 'column',
  gap: '3',
  fontSize: 'sm',
  lineHeight: '1.6',
  color: 'fg',
  '& h1, & h2, & h3, & h4, & h5, & h6': {
    fontWeight: 'semibold',
    marginTop: '2',
    color: 'fg',
  },
  '& h1': { fontSize: 'xl' },
  '& h2': { fontSize: 'lg' },
  '& h3': { fontSize: 'md' },
  '& h4, & h5, & h6': { fontSize: 'sm' },
  '& p': {
    margin: '0',
  },
  '& ul, & ol': {
    paddingLeft: '5',
    display: 'flex',
    flexDirection: 'column',
    gap: '1',
  },
  '& ul': { listStyleType: 'disc' },
  '& ol': { listStyleType: 'decimal' },
  '& li': { paddingLeft: '0.5' },
  '& a': {
    color: 'accent',
    textDecoration: 'underline',
    _hover: { textDecoration: 'none' },
  },
  '& code': {
    fontFamily: 'mono',
    fontSize: '0.9em',
    paddingX: '1',
    paddingY: '0.5',
    borderRadius: 'sm',
    backgroundColor: 'surface',
    border: '1px solid',
    borderColor: 'border',
  },
  '& pre': {
    padding: '3',
    borderRadius: 'sm',
    backgroundColor: 'surface',
    border: '1px solid',
    borderColor: 'border',
    overflowX: 'auto',
  },
  '& pre code': {
    padding: '0',
    border: 'none',
    background: 'transparent',
    fontSize: 'sm',
  },
  '& blockquote': {
    borderLeft: '3px solid',
    borderColor: 'border',
    paddingLeft: '3',
    color: 'fg',
    opacity: '0.85',
    fontStyle: 'italic',
  },
  '& table': {
    borderCollapse: 'collapse',
    fontSize: 'sm',
  },
  '& th, & td': {
    border: '1px solid',
    borderColor: 'border',
    paddingX: '2',
    paddingY: '1',
    textAlign: 'left',
  },
  '& th': {
    backgroundColor: 'surface',
    fontWeight: 'semibold',
  },
  '& hr': {
    border: 'none',
    borderTop: '1px solid',
    borderColor: 'border',
    marginY: '2',
  },
  '& img': {
    maxWidth: '100%',
    height: 'auto',
  },
})

interface MarkdownProps {
  source: string
  className?: string
}

export function Markdown({ source, className }: MarkdownProps) {
  return (
    <div className={className ? `${proseStyle} ${className}` : proseStyle}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeSanitize]}
      >
        {source}
      </ReactMarkdown>
    </div>
  )
}
