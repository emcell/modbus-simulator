import { graphql } from "./graphql";

export const ActiveContextQuery = graphql(`
  query ActiveContext {
    activeContext {
      id
      name
      active
      tcp { enabled bind port }
      rtu { enabled device baudRate parity dataBits stopBits virtualSerialId }
      devices {
        id
        name
        slaveId
        deviceTypeId
        hasBehaviorOverrides
        lastReadAtMs
        lastWriteAtMs
        registerActivity { registerId lastReadAtMs lastWriteAtMs }
        registerValues { registerId value { dataType value } }
        effectiveBehavior {
          disabledFunctionCodes maxRegistersPerRequest
          missingFullBlock missingPartialBlock responseDelayMs
        }
      }
    }
    contexts { id name active }
    deviceTypes {
      id name description
      behavior {
        disabledFunctionCodes maxRegistersPerRequest
        missingFullBlock missingPartialBlock responseDelayMs
      }
      registers {
        id kind address name description
        dataType encoding byteLength
        defaultValue { dataType value }
      }
    }
    virtualSerials { id slavePath symlinkPath inUse }
    transportStatus {
      tcp { state description error }
      rtu { state description error }
    }
  }
`);

export const CreateContextMutation = graphql(`
  mutation CreateContext($name: String!) {
    createContext(name: $name) { id name }
  }
`);

export const SwitchContextMutation = graphql(`
  mutation SwitchContext($id: ID!) {
    switchContext(id: $id)
  }
`);

export const DeleteContextMutation = graphql(`
  mutation DeleteContext($id: ID!) {
    deleteContext(id: $id)
  }
`);

export const ExportContextMutation = graphql(`
  mutation ExportContext($id: ID!) {
    exportContext(id: $id)
  }
`);

export const ImportContextMutation = graphql(`
  mutation ImportContext($data: String!) {
    importContext(data: $data) { id name }
  }
`);

export const CreateDeviceTypeMutation = graphql(`
  mutation CreateDeviceType($input: CreateDeviceTypeInput!) {
    createDeviceType(input: $input) { id name }
  }
`);

export const RenameDeviceTypeMutation = graphql(`
  mutation RenameDeviceType($id: ID!, $name: String!, $description: String) {
    renameDeviceType(id: $id, name: $name, description: $description) { id name description }
  }
`);

export const DeleteDeviceTypeMutation = graphql(`
  mutation DeleteDeviceType($id: ID!) {
    deleteDeviceType(id: $id)
  }
`);

export const ExportDeviceTypeMutation = graphql(`
  mutation ExportDeviceType($id: ID!) {
    exportDeviceType(id: $id)
  }
`);

export const ImportDeviceTypeMutation = graphql(`
  mutation ImportDeviceType($data: String!) {
    importDeviceType(data: $data) { id name }
  }
`);

export const ImportVarmecoCsvMutation = graphql(`
  mutation ImportVarmecoCsv($name: String!, $description: String, $data: String!) {
    importVarmecoCsv(name: $name, description: $description, data: $data) { id name }
  }
`);

export const UpsertRegisterMutation = graphql(`
  mutation UpsertRegister($deviceTypeId: ID!, $input: RegisterInput!) {
    upsertRegister(deviceTypeId: $deviceTypeId, input: $input) { id }
  }
`);

export const DeleteRegisterMutation = graphql(`
  mutation DeleteRegister($id: ID!) {
    deleteRegister(id: $id)
  }
`);

export const UpdateBehaviorMutation = graphql(`
  mutation UpdateBehavior($deviceTypeId: ID!, $input: BehaviorInput!) {
    updateBehavior(deviceTypeId: $deviceTypeId, input: $input) {
      disabledFunctionCodes maxRegistersPerRequest
      missingFullBlock missingPartialBlock responseDelayMs
    }
  }
`);

export const CreateDeviceMutation = graphql(`
  mutation CreateDevice($input: CreateDeviceInput!) {
    createDevice(input: $input) { id name slaveId }
  }
`);

export const DeleteDeviceMutation = graphql(`
  mutation DeleteDevice($id: ID!) { deleteDevice(id: $id) }
`);

export const SetRegisterValueMutation = graphql(`
  mutation SetRegisterValue($deviceId: ID!, $registerId: ID!, $value: ValueInput!) {
    setRegisterValue(deviceId: $deviceId, registerId: $registerId, value: $value) {
      registerId value { dataType value }
    }
  }
`);

export const ConfigureTcpMutation = graphql(`
  mutation ConfigureTcp($enabled: Boolean!, $bind: String!, $port: Int!) {
    configureTcp(enabled: $enabled, bind: $bind, port: $port)
  }
`);

export const ConfigureRtuMutation = graphql(`
  mutation ConfigureRtu(
    $enabled: Boolean!, $device: String!, $baudRate: Int!,
    $parity: String!, $dataBits: Int!, $stopBits: Int!,
    $virtualSerialId: ID
  ) {
    configureRtu(
      enabled: $enabled, device: $device, baudRate: $baudRate,
      parity: $parity, dataBits: $dataBits, stopBits: $stopBits,
      virtualSerialId: $virtualSerialId
    )
  }
`);

export const CreateVirtualSerialMutation = graphql(`
  mutation CreateVirtualSerial($symlinkPath: String) {
    createVirtualSerial(symlinkPath: $symlinkPath) {
      id slavePath symlinkPath
    }
  }
`);

export const RemoveVirtualSerialMutation = graphql(`
  mutation RemoveVirtualSerial($id: ID!) {
    removeVirtualSerial(id: $id)
  }
`);
