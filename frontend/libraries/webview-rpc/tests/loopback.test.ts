import { createLoopbackPair } from '../src/testing';
import { rpcContract } from './contract';

rpcContract('the loopback double', createLoopbackPair);
